
# simple_proxy

## 项目简介

`simple_proxy` 是一个基于 [Pingora](https://github.com/cloudflare/pingora) 实现的双写反向代理服务器。它能够将收到的 HTTP 请求同时转发到两个后端服务（`server.rs` 和 `server_1.rs`），实现主备或数据双写等场景。该项目适合需要高可用、数据一致性或迁移验证的场景。

## 主要特性
- 基于 Rust 语言开发，性能优异，安全可靠
- 使用 Pingora 框架实现高性能反向代理
- 支持 HTTP 请求的双写（主后端+副本后端）
- 后端服务基于 Axum 框架，内置用户管理等REST接口
- 便于扩展和二次开发

## 技术栈
- **Rust 2021/2024**
- **Pingora**：高性能异步代理框架
- **Axum**：现代化 Rust Web 框架
- **tokio**：异步运行时
- **reqwest**：HTTP 客户端
- **tracing**：日志与追踪

## 目录结构
- `src/main.rs`：代理服务入口，监听 8080 端口
- `src/lib.rs`：双写代理核心逻辑
- `examples/server.rs`：后端服务1，监听 127.0.0.1:3000
- `examples/server_1.rs`：后端服务2，监听 127.0.0.1:3001

## 快速开始

### 1. 启动后端服务
分别在两个终端运行：

```bash
cargo run --example server      # 启动 127.0.0.1:3000
```

```bash
cargo run --example server_1    # 启动 127.0.0.1:3001
```

### 2. 启动代理服务

```bash
cargo run                       # 默认监听 0.0.0.0:8080
```

### 3. 测试请求

向代理发送 HTTP 请求，代理会将请求转发到两个后端服务：

```bash
curl -X POST http://127.0.0.1:8080/users -d '{"name":"张三","email":"zhangsan@example.com","password":"123456"}' -H 'Content-Type: application/json'
```

你可以通过访问 `http://127.0.0.1:3000/users` 和 `http://127.0.0.1:3001/users` 查看两个后端的数据。

## 典型应用场景
- 数据库/服务迁移的双写验证
- 主备数据一致性校验
- 灾备演练与切换

## 贡献与开发
欢迎提交 issue 和 PR，建议遵循 Rust 代码规范与最佳实践。

## License

MIT
