use async_trait::async_trait;
use http::HeaderName;
use pingora::{
    http::{RequestHeader, ResponseHeader},
    prelude::HttpPeer,
    proxy::{ProxyHttp, Session},
};
use reqwest::Url;
use std::collections::HashSet;
use std::sync::Mutex;
use tracing::info;
// pub struct SimpleProxy {}

// pub struct CopyProxy {}
pub struct DualWriteProxy {
    pub executed_requests: Mutex<HashSet<String>>,
}

#[async_trait]
impl ProxyHttp for DualWriteProxy {
    type CTX = ();

    fn new_ctx(&self) -> Self::CTX {}

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>, Box<pingora::Error>> {
        // 创建上游服务器
        let peer1 = HttpPeer::new("127.0.0.1:3000", false, "localhost".to_string());

        // 返回第一个peer作为主要的上游服务器
        Ok(Box::new(peer1))
    }

    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        upstream_request: &mut RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<(), Box<pingora::Error>>
    where
        Self::CTX: Send + Sync,
    {
        upstream_request.insert_header(HeaderName::from_static("user-content"), "dual-write")?;

        // 检查是否已经执行过双写（通过请求头标记）
        let dual_write_header = HeaderName::from_static("x-dual-write-executed");
        if !_session
            .req_header()
            .headers
            .contains_key(&dual_write_header)
        {
            // 标记已执行
            _session
                .req_header_mut()
                .insert_header(dual_write_header, "true")?;

            // 启动后台任务向第二个服务器发送请求
            let scheme = "http";
            let host = "127.0.0.1:3001";
            let path_and_query = _session.req_header().uri.to_string();
            let request_uri_string = format!("{scheme}://{host}{path_and_query}");
            let request_uri = Url::parse(&request_uri_string).expect("无法解析为合法URL");
            let request_method = _session.req_header().method.clone();
            let request_headers = _session.req_header().headers.clone();

            // 尝试读取请求体，如果失败则使用空字节
            let request_body_bytes = _session
                .read_request_body()
                .await
                .unwrap_or_default()
                .unwrap_or_default();

            tokio::spawn(async move {
                info!(
                    "Sending duplicate request to peer2: {:?}",
                    request_uri.to_string()
                );

                // 创建不带代理的客户端
                let client = reqwest::Client::builder().no_proxy().build().unwrap();

                let url = request_uri;
                info!("url: {:?}", url);
                info!("method: {:?}", request_method);
                info!("headers: {:?}", request_headers);

                let response = client
                    .request(request_method, url)
                    .headers(request_headers)
                    .body(request_body_bytes)
                    .send()
                    .await;

                info!("response: {:?}", response);
                match response {
                    Ok(resp) => {
                        info!("status: {:?}", resp.status());
                        info!("headers: {:?}", resp.headers());
                        match resp.text().await {
                            Ok(text) => info!("response from 3001: {:?}", text),
                            Err(e) => info!("error reading response: {:?}", e),
                        }
                    }
                    Err(e) => info!("error sending to 3001: {:?}", e),
                }
            });
        }

        Ok(())
    }

    fn upstream_response_filter(
        &self,
        _session: &mut Session,
        upstream_response: &mut ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<(), Box<pingora::Error>> {
        upstream_response
            .insert_header(HeaderName::from_static("user-content"), "response by kevin")?;
        Ok(())
    }
}
