# UNIQL Competitive Strategy & Defensive Plan
**Date:** 2026-03-18
**Based on:** 5 parallel deep research tracks (escape hatches, correlation algorithms, NL-to-query, alert-driven investigation, competitor analysis)

---

## Executive Summary

5 araştırma agent'ının bulguları tek bir sonuca işaret ediyor: **UNIQL doğru yerde, doğru zamanda, ama yanlış mesajla.** "Yeni bir query language" satılmaz — "compiler-verified, multi-backend query intelligence layer" satılır.

### Pazar Durumu
- CNCF Query Standardization Working Group aktif, spec yazıyor ama **implementation yok**
- Logz.io "unified observability" denedi, **başarısız oldu** (migration cost çok yüksek)
- Cribl "search-in-place" ile $3.5B valuation, **data migration yapmama** stratejisi kazanıyor
- ServiceNow UQL, Observe OPAL, Datadog DDSQL — hepsi **vendor-locked**
- Açık kaynak, portable, transpiler-based unified query language **henüz yok**

### Kritik Bulgular

| Eleştiri | Araştırma Bulgusu | Savunma Stratejisi |
|----------|-------------------|-------------------|
| "PromQL öğrenilir" | %25 mühendis observability query dillerini çok karmaşık buluyor (Chronosphere survey). NL→PromQL direct accuracy: %2.6 | NL→UNIQL→PromQL two-hop (%75-85 achievable) |
| "Abstraction tax" | Grafana Builder/Code desync sorunu yaşıyor. SQLAlchemy compositional text() gold standard | NATIVE clause + explain + transparan çeviri |
| "%100 coverage imkansız" | Hibernate native: %10-20, Prisma TypedSQL ayrı feature olarak çıktı | Compositional NATIVE() — SQLAlchemy modeli |
| "Korrelasyon oyuncak" | O(m×l) nested loop. Hash join: **11,131x** improvement (CrateDB benchmark) | Hash-partitioned time-windowed join |
| "Kimin problemi?" | Alert-driven investigation: PagerDuty, Grafana Sift, Shoreline Op Packs hepsi aynı pattern | AETHERIS entegrasyonu: alert → auto-UNIQL → context |
| "ChatGPT de yapar" | NL→PromQL accuracy %2.6 (basic), %69 (with KG). %15-30 error rate persistent | LLM generates, UNIQL validates — deterministic correctness |

---

## Defensive Plan: 6 Aksiyonun Detayı

### Aksiyon 1: NATIVE Clause (Escape Hatch)

**Araştırma temeli:** SQLAlchemy'nin compositional `text()` pattern'i, Calcite'in partial pushdown'u, Prisma'nın TypedSQL evrimi.

**Tasarım kararları:**
- Compositional (tüm query değil, parça bazlı) — SQLAlchemy modeli
- Tek resmi escape hatch — GraphQL'in fragmentation'ından kaçın
- AST node olarak kalır — Grafana'nın round-trip failure'ından kaçın
- Explicit boundary — SQLAlchemy 2.0'ın implicit coercion deprecation dersi

**Syntax:**
```sql
-- Tüm query native
FROM metrics NATIVE "label_replace(up{job='api'}, 'host', '$1', 'instance', '(.*):.*')"

-- Partial: sadece WHERE içinde native fragment
FROM metrics
WHERE __name__ = "http_requests_total"
  AND NATIVE("promql", "job=~'api.*'")
COMPUTE rate(value, 5m)

-- Backend-specific aggregation
FROM metrics
WHERE __name__ = "cpu"
COMPUTE NATIVE("promql", "histogram_quantile(0.99, rate(cpu_bucket[5m]))")
```

**Transpiler davranışı:**
- `NATIVE("promql", expr)` → hedef backend promql ise doğrudan geçir
- Hedef backend farklıysa (logql, logsql) → TranspileError: "Native PromQL fragment cannot be transpiled to LogQL"
- Backend belirtilmezse → hedef backend'in diline assume et

**Pushdown reporting (Calcite modeli):**
```json
{
  "transpiled": "rate(http_requests_total{job=~'api.*'}[5m])",
  "pushed_down": ["__name__", "rate", "5m"],
  "native_passthrough": ["job=~'api.*'"],
  "client_side": []
}
```

---

### Aksiyon 2: Hash Join Correlator

**Araştırma temeli:** ClickHouse hash join (250M lookups/sec), CrateDB 11,131x improvement, Flink keyBy+window pattern, ASOF JOIN semantics.

**Algoritma: Hash-Partitioned Time-Windowed Join**

```
Phase 1 — Build (smaller side, typically metrics):
  HashMap<CompositeKey(join_fields), Vec<Entry>>
  O(m) build time

Phase 2 — Sort within buckets:
  Her bucket'ı timestamp_epoch'a göre sırala
  Genelde zaten sıralı gelir (backend'lerden)

Phase 3 — Probe (larger side, typically logs):
  Her log entry için:
    1. CompositeKey hesapla → O(1) hash lookup
    2. Bucket içinde binary search (time window) → O(log k)
    3. Window içindeki match'leri emit et
```

**Beklenen performans:**

| Senaryo | Şimdiki (nested loop) | Yeni (hash join) |
|---------|----------------------|------------------|
| 1K × 10K | 10M karşılaştırma | ~11K ops |
| 10K × 100K | 1B karşılaştırma | ~110K ops |
| 100K × 1M | 100B karşılaştırma | ~1.1M ops |

**Ek özellikler:**
- `CORRELATE ON host WITHIN 60s MODE closest` → ASOF semantics (sadece en yakın match)
- `CORRELATE ON host WITHIN 60s MODE all` → mevcut davranış (tüm match'ler)
- Memory guard: bucket boyutu threshold aşarsa warning

---

### Aksiyon 3: NL→UNIQL Layer

**Araştırma temeli:** Honeycomb %94 accuracy (JSON DSL target), PromAssistant %69 (PromQL direct), DIN-SQL self-correction %10 recovery, Vanna RAG flywheel.

**Neden NL→UNIQL→Backend, NL→PromQL değil:**
1. LLM'ler SQL benzeri dillerde çok daha iyi (%80-90 vs %2.6-69 PromQL)
2. UNIQL type system %100 deterministic validation sağlar
3. Tek NL pipeline tüm backend'lere hizmet eder (PromQL, LogQL, LogsQL)
4. Her başarılı query training data olur (Vanna flywheel)

**Mimari:**
```
User NL input
  → Schema context (metric names, labels from backend introspection)
  → LLM generates UNIQL candidate
  → UNIQL compiler validates (parse → bind → normalize)
  → If invalid: error feedback → LLM retry (DIN-SQL self-correction)
  → If valid: transpile → execute
  → Show UNIQL + native query to user (Microsoft/Honeycomb pattern)
```

**Achievable accuracy:** %75-85 (SQL training data advantage + constrained domain + type validation)

**Implementation:** Phase olarak — v1.x'te API endpoint, v2.0'da AETHERIS chatbox entegrasyonu.

---

### Aksiyon 4: Alert-Driven Investigation Packs

**Araştırma temeli:** Grafana Sift (auto-extract labels → auto-run checks), Shoreline Op Packs (Terraform-based diagnostic DAGs), PagerDuty Past Incidents, Datadog Notebook Templates.

**Konsept: UNIQL Investigation Packs**

```sql
-- Bir alert tipi için investigation pack tanımı
DEFINE investigate_high_cpu(service, host, start_time) = (
  -- 1. Affected metric trend
  FROM metrics
  WHERE __name__ = "cpu_usage" AND service = $service AND host = $host
  WITHIN last 30m FROM $start_time
  COMPUTE rate(value, 5m);

  -- 2. Related error logs
  FROM logs
  WHERE service = $service AND level = "error"
  WITHIN last 15m FROM $start_time;

  -- 3. Cross-signal correlation
  FROM metrics, logs
  WHERE metrics.__name__ = "cpu_usage" AND logs.level = "error"
    AND metrics.host = $host AND logs.host = $host
  CORRELATE ON host WITHIN 60s
)
```

**AETHERIS entegrasyonu:**
```
Alert fires (host=srv-01, service=api, severity=critical)
  → AETHERIS event handler
  → Auto-select investigation pack by alert type
  → Parameterize: investigate_high_cpu("api", "srv-01", "2026-03-18T14:30:00Z")
  → Execute 3 UNIQL queries parallel
  → Present correlated results in investigation view
  → Operator sees context, clicks "acknowledge" or "escalate"
```

**Progressive disclosure:**
- **Tier-1 NOC:** Tıkla, sonuçları gör, hiç query yazma
- **Tier-2 SRE:** Investigation pack'in UNIQL source'unu gör, modifiye et
- **Tier-3 Platform:** Yeni investigation pack DEFINE et, platforma ekle

---

### Aksiyon 5: Playground'da Dual View

**Araştırma temeli:** Grafana Builder/Code desync problemi (değişiklikler kaybolur), Honeycomb'un query builder + NL combo'su.

**Tasarım: Three-Pane View**
```
┌─────────────────────┬──────────────────────┬──────────────────────┐
│      UNIQL          │    Native Query       │    Result            │
│                     │                       │                      │
│ FROM metrics        │ rate(                 │ ┌────┬───────┐       │
│ WHERE __name__ =    │   http_requests_total │ │host│ value  │       │
│   "http_requests_   │   {env="prod"}        │ ├────┼───────┤       │
│    total"           │   [5m]                │ │srv1│ 42.3   │       │
│   AND env = "prod"  │ )                     │ │srv2│ 18.7   │       │
│ COMPUTE rate(       │                       │ └────┴───────┘       │
│   value, 5m)        │                       │                      │
└─────────────────────┴──────────────────────┴──────────────────────┘
```

- Sol panel edit → sağ panel anında güncellenir (transpile <1ms)
- Sağ panel read-only (native output)
- **Round-trip korunur** — her zaman UNIQL source of truth

---

### Aksiyon 6: CNCF Alignment

**Araştırma temeli:** CNCF TAG Observability QLS working group — spec yazıyor, implementation yok. SQL basis öneriyorlar.

**Strateji:**
- UNIQL'in SQL-familiar syntax'ı zaten CNCF yönüyle uyumlu
- Working group'a katıl, UNIQL'i reference implementation olarak sun
- İlk mover advantage: spec finalize olmadan working implementation sun

---

## Implementation Priority

| # | Aksiyon | Efor | Etki | Eleştiriyi Gömme |
|---|---------|------|------|-----------------|
| 1 | NATIVE clause | 2-3 gün | Yüksek | "%100 coverage imkansız" |
| 2 | Hash join correlator | 1-2 gün | Yüksek | "Korrelasyon oyuncak" |
| 3 | Playground dual view | Phase 4 | Orta | "Debug zorlaşır" |
| 4 | Investigation packs | AETHERIS tarafı | Çok yüksek | "Kimin problemi?" |
| 5 | NL→UNIQL | v1.x/v2.0 | Çok yüksek | "Neden yeni dil?" + "ChatGPT de yapar" |
| 6 | CNCF alignment | Ongoing | Stratejik | "Standard olmaz" |

---

## Key Metrics to Track

| Metrik | Hedef | Neden |
|--------|-------|-------|
| PromQL feature coverage | %90+ (NATIVE ile %100) | "%100 imkansız" eleştirisini gömer |
| Correlation benchmark | 100K × 1M < 200ms | "Oyuncak" eleştirisini gömer |
| NL→UNIQL accuracy | %80+ | "ChatGPT de yapar" eleştirisini gömer |
| Investigation pack MTTR reduction | %40+ | "Kimin problemi" eleştirisini gömer |
| CNCF spec alignment score | %80+ | "Standard olmaz" eleştirisini gömer |

---

## Positioning Statement

> UNIQL is the **compiler-verified, multi-backend query intelligence layer** for observability.
> It does not replace PromQL — it makes PromQL accessible to everyone,
> validates LLM-generated queries deterministically,
> and powers automated investigation across any backend.

---

## Sources
(Her araştırma agent'ının kaynak listesi ayrıca mevcut — toplam 80+ kaynak analiz edildi)
