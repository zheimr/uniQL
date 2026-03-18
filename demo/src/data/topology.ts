export interface TopologyNode {
  id: string;
  label: string;
  type: 'firewall' | 'switch' | 'collector' | 'database' | 'engine';
  x: number;
  y: number;
}

export interface TopologyEdge {
  from: string;
  to: string;
  label: string;
  bidirectional?: boolean;
}

export const nodes: TopologyNode[] = [
  { id: 'fortigate', label: 'FortiGate\nFirewall', type: 'firewall', x: 400, y: 80 },
  { id: 'switch-1', label: 'Cisco\nSwitch-1', type: 'switch', x: 180, y: 220 },
  { id: 'switch-2', label: 'Cisco\nSwitch-2', type: 'switch', x: 620, y: 220 },
  { id: 'syslog', label: 'Syslog\nCollector', type: 'collector', x: 400, y: 340 },
  { id: 'victoria-metrics', label: 'Victoria\nMetrics', type: 'database', x: 140, y: 460 },
  { id: 'victoria-logs', label: 'Victoria\nLogs', type: 'database', x: 660, y: 460 },
  { id: 'uniql', label: 'UNIQL\nEngine', type: 'engine', x: 400, y: 560 },
];

export const edges: TopologyEdge[] = [
  { from: 'fortigate', to: 'switch-1', label: 'SNMP' },
  { from: 'fortigate', to: 'switch-2', label: 'SNMP' },
  { from: 'switch-1', to: 'syslog', label: 'Syslog' },
  { from: 'switch-2', to: 'syslog', label: 'Syslog' },
  { from: 'syslog', to: 'victoria-logs', label: 'Logs' },
  { from: 'fortigate', to: 'victoria-metrics', label: 'Metrics' },
  { from: 'uniql', to: 'victoria-metrics', label: 'PromQL', bidirectional: true },
  { from: 'uniql', to: 'victoria-logs', label: 'LogsQL', bidirectional: true },
];

export const nodeColors: Record<string, { bg: string; border: string; text: string }> = {
  firewall: { bg: '#fee2e2', border: '#ef4444', text: '#991b1b' },
  switch: { bg: '#dbeafe', border: '#3b82f6', text: '#1e3a8a' },
  collector: { bg: '#fef3c7', border: '#f59e0b', text: '#92400e' },
  database: { bg: '#d1fae5', border: '#10b981', text: '#065f46' },
  engine: { bg: '#e0e7ff', border: '#6366f1', text: '#3730a3' },
};
