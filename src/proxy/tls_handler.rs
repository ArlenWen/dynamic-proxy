use rustls::{ServerConfig, server::ResolvesServerCert};
use rustls_pemfile::{certs, pkcs8_private_keys};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::{TlsAcceptor, server::TlsStream};
use tokio::net::TcpStream;
use std::io::BufReader;
use std::fs::File;
use std::sync::Arc;
use std::collections::HashMap;
use anyhow::{Result, Context};
use tracing::{debug, error, warn, info};

pub struct TlsHandler {
    acceptor: Option<TlsAcceptor>,
    sni_routing_enabled: bool,
    cert_resolver: Option<Arc<MultiCertResolver>>,
}

// Custom certificate resolver for SNI-based certificate selection
#[derive(Debug, Clone)]
pub struct MultiCertResolver {
    certificates: HashMap<String, Arc<rustls::sign::CertifiedKey>>,
    default_cert: Option<Arc<rustls::sign::CertifiedKey>>,
    last_sni: Arc<std::sync::Mutex<Option<String>>>,
}

impl MultiCertResolver {
    pub fn new() -> Self {
        Self {
            certificates: HashMap::new(),
            default_cert: None,
            last_sni: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn add_certificate(&mut self, hostname: String, cert_key: Arc<rustls::sign::CertifiedKey>) {
        if self.default_cert.is_none() {
            self.default_cert = Some(cert_key.clone());
        }
        self.certificates.insert(hostname, cert_key);
    }

    pub fn set_default_certificate(&mut self, cert_key: Arc<rustls::sign::CertifiedKey>) {
        self.default_cert = Some(cert_key);
    }

    pub fn get_last_sni(&self) -> Option<String> {
        self.last_sni.lock().unwrap().clone()
    }
}

impl ResolvesServerCert for MultiCertResolver {
    fn resolve(&self, client_hello: rustls::server::ClientHello) -> Option<Arc<rustls::sign::CertifiedKey>> {
        // Extract SNI from client hello
        if let Some(server_name) = client_hello.server_name() {
            let hostname = server_name.to_string();
            debug!("SNI hostname detected: {}", hostname);

            // Store the SNI for later retrieval
            if let Ok(mut last_sni) = self.last_sni.lock() {
                *last_sni = Some(hostname.to_string());
            }

            // Try exact match first
            if let Some(cert) = self.certificates.get(&hostname) {
                debug!("Found exact certificate match for: {}", hostname);
                return Some(cert.clone());
            }

            // Try wildcard matching
            for (cert_hostname, cert) in &self.certificates {
                if cert_hostname.starts_with("*.") {
                    let domain = &cert_hostname[2..];
                    if hostname.ends_with(domain) {
                        debug!("Found wildcard certificate match for: {} -> {}", hostname, cert_hostname);
                        return Some(cert.clone());
                    }
                }
            }
        }

        // Return default certificate if no match found
        if let Some(default) = &self.default_cert {
            debug!("Using default certificate");
            Some(default.clone())
        } else {
            warn!("No certificates available");
            None
        }
    }
}

impl TlsHandler {
    pub async fn new(cert_path: &str, key_path: &str, sni_routing: bool) -> Result<Self> {
        let (acceptor, cert_resolver) = if std::path::Path::new(cert_path).exists() && std::path::Path::new(key_path).exists() {
            let (acceptor, resolver) = Self::create_tls_acceptor_with_sni(cert_path, key_path, sni_routing).await?;
            (Some(acceptor), Some(resolver))
        } else {
            warn!("TLS certificate or key file not found, TLS will be disabled");
            (None, None)
        };

        Ok(Self {
            acceptor,
            sni_routing_enabled: sni_routing,
            cert_resolver,
        })
    }

    pub async fn new_with_multiple_certs(cert_configs: Vec<(String, String, String)>, sni_routing: bool) -> Result<Self> {
        if cert_configs.is_empty() {
            warn!("No certificate configurations provided, TLS will be disabled");
            return Ok(Self {
                acceptor: None,
                sni_routing_enabled: sni_routing,
                cert_resolver: None,
            });
        }

        let mut cert_resolver = MultiCertResolver::new();

        for (hostname, cert_path, key_path) in cert_configs {
            if std::path::Path::new(&cert_path).exists() && std::path::Path::new(&key_path).exists() {
                let cert_key = Self::load_certificate(&cert_path, &key_path).await?;
                cert_resolver.add_certificate(hostname.clone(), cert_key);
                info!("Loaded certificate for hostname: {}", hostname);
            } else {
                warn!("Certificate or key file not found for hostname {}: {} / {}", hostname, cert_path, key_path);
            }
        }

        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(cert_resolver.clone()));

        let acceptor = TlsAcceptor::from(Arc::new(config));

        Ok(Self {
            acceptor: Some(acceptor),
            sni_routing_enabled: sni_routing,
            cert_resolver: Some(Arc::new(cert_resolver)),
        })
    }

    async fn create_tls_acceptor_with_sni(cert_path: &str, key_path: &str, _sni_routing: bool) -> Result<(TlsAcceptor, Arc<MultiCertResolver>)> {
        let cert_key = Self::load_certificate(cert_path, key_path).await?;

        let mut cert_resolver = MultiCertResolver::new();
        cert_resolver.set_default_certificate(cert_key);

        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(cert_resolver.clone()));

        let acceptor = TlsAcceptor::from(Arc::new(config));
        Ok((acceptor, Arc::new(cert_resolver)))
    }

    async fn load_certificate(cert_path: &str, key_path: &str) -> Result<Arc<rustls::sign::CertifiedKey>> {
        // 读取证书文件
        let cert_file = File::open(cert_path)
            .with_context(|| format!("Failed to open certificate file: {}", cert_path))?;
        let mut cert_reader = BufReader::new(cert_file);
        let cert_chain: Vec<CertificateDer> = certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| "Failed to parse certificate file")?;

        // 读取私钥文件
        let key_file = File::open(key_path)
            .with_context(|| format!("Failed to open private key file: {}", key_path))?;
        let mut key_reader = BufReader::new(key_file);
        let mut keys: Vec<_> = pkcs8_private_keys(&mut key_reader)
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| "Failed to parse private key file")?;

        if keys.is_empty() {
            anyhow::bail!("No private keys found in key file");
        }

        let private_key = PrivateKeyDer::Pkcs8(keys.remove(0));

        // 创建签名密钥
        let signing_key = rustls::crypto::ring::sign::any_supported_type(&private_key)
            .with_context(|| "Failed to create signing key")?;

        let certified_key = rustls::sign::CertifiedKey::new(cert_chain, signing_key);
        Ok(Arc::new(certified_key))
    }

    pub async fn handle_connection(&self, stream: TcpStream) -> Result<Option<(TlsStream<TcpStream>, Option<String>)>> {
        if let Some(acceptor) = &self.acceptor {
            debug!("Attempting TLS handshake");

            match acceptor.accept(stream).await {
                Ok(tls_stream) => {
                    debug!("TLS handshake successful");

                    // 提取SNI信息 - 现在通过TLS连接获取
                    let sni_hostname = if self.sni_routing_enabled {
                        self.extract_sni_from_connection(&tls_stream).await
                    } else {
                        None
                    };

                    Ok(Some((tls_stream, sni_hostname)))
                }
                Err(e) => {
                    error!("TLS handshake failed: {}", e);
                    Err(anyhow::anyhow!("TLS handshake failed: {}", e))
                }
            }
        } else {
            // TLS未配置，返回None表示应该使用普通TCP连接
            Ok(None)
        }
    }

    async fn extract_sni_from_connection(&self, _tls_stream: &TlsStream<TcpStream>) -> Option<String> {
        // 从证书解析器获取最后记录的SNI
        if let Some(resolver) = &self.cert_resolver {
            resolver.get_last_sni()
        } else {
            None
        }
    }



    pub fn is_tls_enabled(&self) -> bool {
        self.acceptor.is_some()
    }

    pub fn supports_sni_routing(&self) -> bool {
        self.sni_routing_enabled && self.acceptor.is_some()
    }
}

// SNI提取的辅助结构
pub struct SniExtractor {
    hostname: Option<String>,
}

impl SniExtractor {
    pub fn new() -> Self {
        Self { hostname: None }
    }

    pub fn extract_from_client_hello(&mut self, data: &[u8]) -> Option<String> {
        // 改进的SNI提取实现
        // 更健壮的TLS ClientHello消息解析

        if data.len() < 43 {
            debug!("TLS data too short: {} bytes", data.len());
            return None;
        }

        // 检查是否是TLS握手消息 (0x16)
        if data[0] != 0x16 {
            debug!("Not a TLS handshake message: 0x{:02x}", data[0]);
            return None;
        }

        // 检查TLS版本 (应该是3.x)
        if data[1] != 0x03 {
            debug!("Unexpected TLS version: {}.{}", data[1], data[2]);
            return None;
        }

        // 检查是否是ClientHello (0x01)
        if data[5] != 0x01 {
            debug!("Not a ClientHello message: 0x{:02x}", data[5]);
            return None;
        }

        // 验证消息长度
        let record_length = u16::from_be_bytes([data[3], data[4]]) as usize;
        if data.len() < 5 + record_length {
            debug!("Incomplete TLS record: expected {}, got {}", 5 + record_length, data.len());
            return None;
        }

        // 跳过固定字段，查找扩展
        let mut offset = 43;

        // 安全地跳过会话ID
        if offset >= data.len() {
            debug!("Offset {} exceeds data length {}", offset, data.len());
            return None;
        }
        let session_id_len = data[offset] as usize;
        offset += 1 + session_id_len;

        // 安全地跳过密码套件
        if offset + 2 > data.len() {
            debug!("Not enough data for cipher suites at offset {}", offset);
            return None;
        }
        let cipher_suites_len = u16::from_be_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2 + cipher_suites_len;

        // 安全地跳过压缩方法
        if offset >= data.len() {
            debug!("Not enough data for compression methods at offset {}", offset);
            return None;
        }
        let compression_methods_len = data[offset] as usize;
        offset += 1 + compression_methods_len;

        // 解析扩展
        if offset + 2 > data.len() {
            debug!("Not enough data for extensions length at offset {}", offset);
            return None;
        }

        let extensions_len = u16::from_be_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;

        if offset + extensions_len > data.len() {
            debug!("Extensions length {} exceeds remaining data", extensions_len);
            return None;
        }

        let extensions_end = offset + extensions_len;
        while offset + 4 <= extensions_end && offset + 4 <= data.len() {
            let ext_type = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let ext_len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
            offset += 4;

            if offset + ext_len > data.len() {
                debug!("Extension length {} exceeds remaining data at offset {}", ext_len, offset);
                break;
            }

            // SNI扩展类型是0x0000
            if ext_type == 0x0000 {
                debug!("Found SNI extension with length {}", ext_len);
                if let Some(hostname) = self.parse_sni_extension(&data[offset..offset + ext_len]) {
                    debug!("Successfully extracted SNI: {}", hostname);
                    return Some(hostname);
                }
            }

            offset += ext_len;
        }

        debug!("No SNI extension found in ClientHello");
        None
    }

    fn parse_sni_extension(&mut self, data: &[u8]) -> Option<String> {
        if data.len() < 5 {
            debug!("SNI extension data too short: {} bytes", data.len());
            return None;
        }

        let list_len = u16::from_be_bytes([data[0], data[1]]) as usize;
        if list_len + 2 > data.len() {
            debug!("SNI list length {} exceeds data length {}", list_len, data.len());
            return None;
        }

        let mut offset = 2;
        while offset + 3 <= list_len + 2 && offset + 3 <= data.len() {
            let name_type = data[offset];
            let name_len = u16::from_be_bytes([data[offset + 1], data[offset + 2]]) as usize;
            offset += 3;

            if offset + name_len > data.len() {
                debug!("SNI name length {} exceeds remaining data at offset {}", name_len, offset);
                break;
            }

            if name_type == 0x00 {
                // 主机名类型
                match String::from_utf8(data[offset..offset + name_len].to_vec()) {
                    Ok(hostname) => {
                        if self.is_valid_hostname(&hostname) {
                            debug!("Valid SNI hostname extracted: {}", hostname);
                            self.hostname = Some(hostname.clone());
                            return Some(hostname);
                        } else {
                            debug!("Invalid hostname format: {}", hostname);
                        }
                    }
                    Err(e) => {
                        debug!("Failed to parse hostname as UTF-8: {}", e);
                    }
                }
            }

            offset += name_len;
        }

        debug!("No valid hostname found in SNI extension");
        None
    }

    fn is_valid_hostname(&self, hostname: &str) -> bool {
        // 基本的主机名验证
        if hostname.is_empty() || hostname.len() > 253 {
            return false;
        }

        // 检查是否包含有效字符
        hostname.chars().all(|c| {
            c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_'
        }) && !hostname.starts_with('.') && !hostname.ends_with('.')
    }

    pub fn get_hostname(&self) -> Option<&String> {
        self.hostname.as_ref()
    }
}
