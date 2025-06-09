use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingRule {
    pub id: String,
    pub name: String,
    pub priority: u32,
    pub conditions: Vec<RoutingCondition>,
    pub actions: Vec<RoutingAction>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoutingCondition {
    SniPrefix { prefix: String },
    SniSuffix { suffix: String },
    SniExact { hostname: String },
    SourceIp { cidr: String },
    DestinationPort { port: u16 },
    Protocol { protocol: String }, // "tcp" or "udp"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoutingAction {
    Forward { backend: String },
    LoadBalance { backends: Vec<String>, method: LoadBalanceMethod },
    Reject { reason: String },
    TlsTerminate { backend: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoadBalanceMethod {
    RoundRobin,
    LeastConnections,
    Random,
    WeightedRoundRobin { weights: Vec<u32> },
}

#[derive(Debug, Clone)]
pub struct RoutingDecision {
    pub action: RoutingAction,
    pub backend_addr: Option<SocketAddr>,
    pub tls_terminate: bool,
    pub rule_id: String,
}

#[derive(Debug, Clone)]
pub struct RoutingContext {
    pub source_addr: SocketAddr,
    pub destination_addr: SocketAddr,
    pub protocol: String,
    pub sni_hostname: Option<String>,
    pub connection_id: String,
}

impl RoutingRule {
    pub fn matches(&self, context: &RoutingContext) -> bool {
        if !self.enabled {
            return false;
        }

        self.conditions.iter().all(|condition| {
            match condition {
                RoutingCondition::SniPrefix { prefix } => {
                    context.sni_hostname
                        .as_ref()
                        .map(|sni| sni.starts_with(prefix))
                        .unwrap_or(false)
                }
                RoutingCondition::SniSuffix { suffix } => {
                    context.sni_hostname
                        .as_ref()
                        .map(|sni| sni.ends_with(suffix))
                        .unwrap_or(false)
                }
                RoutingCondition::SniExact { hostname } => {
                    context.sni_hostname
                        .as_ref()
                        .map(|sni| sni == hostname)
                        .unwrap_or(false)
                }
                RoutingCondition::SourceIp { cidr } => {
                    // 简化实现，实际应该解析CIDR
                    cidr.contains(&context.source_addr.ip().to_string())
                }
                RoutingCondition::DestinationPort { port } => {
                    context.destination_addr.port() == *port
                }
                RoutingCondition::Protocol { protocol } => {
                    context.protocol == *protocol
                }
            }
        })
    }

    pub fn apply(&self, context: &RoutingContext) -> Option<RoutingDecision> {
        if !self.matches(context) {
            return None;
        }

        // 取第一个动作并尝试解析backend地址
        if let Some(action) = self.actions.first() {
            let backend_addr = self.resolve_backend_addr(action);
            Some(RoutingDecision {
                action: action.clone(),
                backend_addr,
                tls_terminate: matches!(action, RoutingAction::TlsTerminate { .. }),
                rule_id: self.id.clone(),
            })
        } else {
            None
        }
    }

    fn resolve_backend_addr(&self, action: &RoutingAction) -> Option<SocketAddr> {
        match action {
            RoutingAction::Forward { backend } => {
                // 尝试解析backend字符串为SocketAddr
                backend.parse().ok()
            }
            RoutingAction::TlsTerminate { backend } => {
                // 尝试解析backend字符串为SocketAddr
                backend.parse().ok()
            }
            RoutingAction::LoadBalance { backends, .. } => {
                // 对于负载均衡，返回第一个可解析的backend
                backends.iter()
                    .find_map(|backend| backend.parse().ok())
            }
            RoutingAction::Reject { .. } => None,
        }
    }
}
