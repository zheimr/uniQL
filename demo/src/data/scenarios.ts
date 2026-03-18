export interface Scenario {
  id: string;
  title: string;
  icon: string;
  device: string;
  query: string;
  description: string;
  backend: 'promql' | 'logql' | 'logsql';
  pack?: string;
  params?: Record<string, string>;
}

export const scenarios: Scenario[] = [
  {
    id: 'snmp-devices',
    title: 'SNMP Device Status',
    icon: '📡',
    device: 'snmp',
    query: `SHOW timeseries FROM victoria
WHERE __name__ = "snmpv2_device_up"`,
    description: '721 SNMP cihazının anlık durumu — AETHERIS NOC gerçek veri',
    backend: 'promql',
  },
  {
    id: 'esxi-cpu',
    title: 'ESXi Host CPU',
    icon: '🖥️',
    device: 'vcenter',
    query: `SHOW timeseries FROM victoria
WHERE __name__ = "vsphere_host_cpu_usage_average"`,
    description: 'Kocaeli BB ESXi host CPU kullanımı — canlı vCenter verisi',
    backend: 'promql',
  },
  {
    id: 'vm-memory',
    title: 'VM Memory Usage',
    icon: '☁️',
    device: 'vcenter',
    query: `SHOW timeseries FROM victoria
WHERE __name__ = "vsphere_vm_mem_usage_average"`,
    description: '370 VM bellek kullanımı — AETHERIS SYS modül verisi',
    backend: 'promql',
  },
  {
    id: 'service-health',
    title: 'Service Health',
    icon: '💚',
    device: 'platform',
    query: `SHOW timeseries FROM victoria
WHERE __name__ = "up"`,
    description: 'Platform servis durumları — tüm AETHERIS bileşenleri',
    backend: 'promql',
  },
  {
    id: 'fortigate-logs',
    title: 'FortiGate Logs',
    icon: '🔥',
    device: 'fortigate',
    query: `SHOW table FROM vlogs
WHERE job = "fortigate"`,
    description: 'FortiGate syslog akışı — AETHERIS SOC log pipeline',
    backend: 'logsql',
  },
  {
    id: 'cross-signal',
    title: 'Cross-Signal RCA',
    icon: '🔗',
    device: 'uniql',
    query: `SHOW timeseries FROM victoria
WHERE __name__ = "vsphere_host_cpu_usage_average"
  AND clustername = "DELLR750_Cluster"`,
    description: 'Cluster bazlı CPU analizi — tek sorgu ile tüm hostlar',
    backend: 'promql',
  },
  {
    id: 'within-range',
    title: 'WITHIN Range',
    icon: '🕐',
    device: 'platform',
    query: `SHOW timeseries FROM victoria
WHERE __name__ = "up"
WITHIN last 1h`,
    description: 'WITHIN time range — 1 saat geriye query_range',
    backend: 'promql',
  },
  {
    id: 'vlogs-query',
    title: 'VLogs Live',
    icon: '📋',
    device: 'fortigate',
    query: `SHOW table FROM vlogs
WHERE job = "fortigate"
WITHIN last 15m`,
    description: 'FortiGate logları — VictoriaLogs LogsQL ile',
    backend: 'logsql',
  },
  {
    id: 'compute-groupby',
    title: 'COMPUTE + GROUP BY',
    icon: '📊',
    device: 'platform',
    query: `FROM metrics
WHERE __name__ = "up"
COMPUTE count()
GROUP BY job`,
    description: 'Servis bazlı aggregation — count by job',
    backend: 'promql',
  },
  {
    id: 'parse-json',
    title: 'PARSE JSON',
    icon: '🔧',
    device: 'fortigate',
    query: `FROM logs
WHERE job = "fortigate"
PARSE json
WITHIN last 5m`,
    description: 'Log parsing pipeline — JSON field extraction',
    backend: 'logsql',
  },
  {
    id: 'define-macro',
    title: 'DEFINE Macro',
    icon: '🔁',
    device: 'vcenter',
    query: `DEFINE high_cpu = __name__ = "vsphere_host_cpu_usage_average"
FROM metrics WHERE high_cpu`,
    description: 'Reusable macro — DEFINE/USE pattern',
    backend: 'promql',
  },
];

export const investigationSteps = [
  {
    id: 1,
    title: 'Alert Tetiklendi',
    icon: '🔴',
    description: 'ESXi host CPU > 85% — AETHERIS vmalert kuralı',
    detail:
      'VictoriaMetrics vmalert kuralı tespit etti: vsphere_host_cpu_usage_average > 85 son 5 dakikadır. Host: r750g01.kocaeli.bel.tr, Cluster: DELLR750_Cluster.',
    query: `SHOW timeseries FROM victoria WHERE __name__ = "vsphere_host_cpu_usage_average"`,
  },
  {
    id: 2,
    title: 'Investigation Pack Başlatıldı',
    icon: '📦',
    description: 'UNIQL "high_cpu" paketi 3 paralel sorgu başlattı',
    detail:
      'high_cpu investigation pack aktif. CPU trend + Top VM by CPU + Host memory sorguları paralel çalıştırıldı.',
    query: null,
  },
  {
    id: 3,
    title: '3 Paralel Sorgu Tamamlandı',
    icon: '⚡',
    description: 'CPU trend | Top VMs | Memory korelasyonu',
    detail:
      'Query 1: vsphere_host_cpu → spike 14:32, %92 peak\nQuery 2: vsphere_vm_cpu top 5 → Gunes_Test_Linux en yüksek\nQuery 3: vsphere_host_mem → %67, normal aralıkta',
    query: `SHOW timeseries FROM victoria WHERE __name__ = "vsphere_vm_cpu_usage_average"`,
  },
  {
    id: 4,
    title: 'Korelasyon Analizi',
    icon: '🧩',
    description: 'Host + VM + zaman eşleşmesi bulundu',
    detail:
      'CORRELATE ON host WITHIN 60s: Host CPU spike (14:32, %92) + VM "Gunes_Test_Linux" CPU (14:31, %100) aynı host üzerinde, aynı zaman dilimi. VM kaynaklı host spike.',
    query: null,
  },
  {
    id: 5,
    title: 'Root Cause Belirlendi',
    icon: '✅',
    description: 'Gunes_Test_Linux VM — CPU runaway, 14:31\'de başladı',
    detail:
      'Root Cause: r750g01.kocaeli.bel.tr üzerindeki Gunes_Test_Linux VM\'i 14:31\'de %100 CPU kullanımına ulaştı ve host-level CPU spike\'a neden oldu. Önerilen aksiyon: VM CPU limiti veya rightsizing.',
    query: null,
  },
];
