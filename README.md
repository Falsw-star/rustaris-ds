# Rustaris  

**Rustaris** 是由 rust 编写的、 ai 驱动的 qq 机器人。她来自上古失落科技文明废土时代，如今是 Falsw 的二女儿。（大女儿是 **Falice** ）  

## Usage

### 初次运行  

程序在 panic 之后会生成一个 `config.json` 文件。请按照自己情况填写。  
```json
{
    // 每次检查消息的间隔。单位：秒
    "heart_beat": 0.5,
    "network": {
        // napcat 中 `Websocket 服务器` 的地址
        "websocket": "ws://192.168.3.38:3005",
        // napcat 中 `Websocket 服务器` 和 `Http 服务器` 的 token 。（请将两个服务器的 token 设为相同）
        "login_token": "rusta",
        // napcat 中 `Http 服务器` 的地址
        "http": "http://192.168.3.38:3004"
    },
    // 各个等级的日志是否要输出到控制台
    "logger": {
        "info": true,
        "warning": true,
        "error": true,
        "chat": true,
        "debug": true,
        "generate_file": false,
        "save_path": null
    },
    "permission": {
        "default": 0,
        "private": 0,
        "admins": [],
        "other": {}
    }
}
```  

然后，在目录下创建 `.env` 文件，填写：
```.env
POSTGRES_USER=bot
POSTGRES_PASSWORD=your_strong_password
POSTGRES_DB=botdb
API_KEY=your-deepseek-api-key
```

再次运行。