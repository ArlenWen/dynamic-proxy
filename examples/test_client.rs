use tokio::net::{TcpStream, UdpSocket};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Dynamic Proxy Test Client");
    println!("========================");

    // 等待代理服务器启动
    tokio::time::sleep(Duration::from_secs(1)).await;

    // 测试TCP连接
    println!("Testing TCP connection...");
    match test_tcp_connection().await {
        Ok(_) => println!("✓ TCP connection test completed"),
        Err(e) => println!("✗ TCP connection test failed: {}", e),
    }

    // 测试UDP连接
    println!("Testing UDP connection...");
    match test_udp_connection().await {
        Ok(_) => println!("✓ UDP connection test completed"),
        Err(e) => println!("✗ UDP connection test failed: {}", e),
    }

    // 测试指标端点
    println!("Testing metrics endpoint...");
    match test_metrics_endpoint().await {
        Ok(_) => println!("✓ Metrics endpoint test completed"),
        Err(e) => println!("✗ Metrics endpoint test failed: {}", e),
    }

    Ok(())
}

async fn test_tcp_connection() -> anyhow::Result<()> {
    // 尝试连接到TCP代理
    match TcpStream::connect("127.0.0.1:18080").await {
        Ok(mut stream) => {
            println!("  Connected to TCP proxy");
            
            // 发送一些测试数据
            let test_data = b"Hello, TCP Proxy!";
            stream.write_all(test_data).await?;
            println!("  Sent: {}", String::from_utf8_lossy(test_data));
            
            // 尝试读取响应（可能会超时，这是正常的）
            let mut buffer = [0; 1024];
            match tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buffer)).await {
                Ok(Ok(n)) if n > 0 => {
                    println!("  Received: {}", String::from_utf8_lossy(&buffer[..n]));
                }
                _ => {
                    println!("  No response received (expected for this test)");
                }
            }
        }
        Err(e) => {
            println!("  Failed to connect: {}", e);
        }
    }
    
    Ok(())
}

async fn test_udp_connection() -> anyhow::Result<()> {
    // 创建UDP socket
    let socket = UdpSocket::bind("127.0.0.1:0").await?;
    
    // 发送数据到UDP代理
    let test_data = b"Hello, UDP Proxy!";
    socket.send_to(test_data, "127.0.0.1:18081").await?;
    println!("  Sent UDP packet: {}", String::from_utf8_lossy(test_data));
    
    // 尝试接收响应
    let mut buffer = [0; 1024];
    match tokio::time::timeout(Duration::from_secs(2), socket.recv_from(&mut buffer)).await {
        Ok(Ok((n, addr))) => {
            println!("  Received UDP response from {}: {}", addr, String::from_utf8_lossy(&buffer[..n]));
        }
        _ => {
            println!("  No UDP response received (expected for this test)");
        }
    }
    
    Ok(())
}

async fn test_metrics_endpoint() -> anyhow::Result<()> {
    // 尝试连接到指标端点
    match TcpStream::connect("127.0.0.1:19090").await {
        Ok(mut stream) => {
            println!("  Connected to metrics endpoint");
            
            // 发送HTTP GET请求
            let request = "GET /metrics HTTP/1.1\r\nHost: 127.0.0.1:19090\r\nConnection: close\r\n\r\n";
            stream.write_all(request.as_bytes()).await?;
            
            // 读取响应
            let mut response = String::new();
            match tokio::time::timeout(Duration::from_secs(5), stream.read_to_string(&mut response)).await {
                Ok(Ok(_)) => {
                    if response.contains("proxy_") {
                        println!("  ✓ Metrics endpoint is working");
                        println!("  Sample metrics found in response");
                    } else {
                        println!("  ⚠ Metrics endpoint responded but no proxy metrics found");
                    }
                }
                _ => {
                    println!("  ✗ Failed to read metrics response");
                }
            }
        }
        Err(e) => {
            println!("  Failed to connect to metrics endpoint: {}", e);
        }
    }
    
    Ok(())
}
