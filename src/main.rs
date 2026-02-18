use std::{sync::{Arc, Mutex}, time::Duration};

use rustaris_ds::{
    CONFIG, DEV, adapters, commands::run_cmds, get_logger, logging::LoggerProvider, objects::Event, set_exit_handler, thinking::{self, Thinker}
};

use tokio::time::sleep;

#[tokio::main]
async fn main() -> anyhow::Result<()> {

    let logger_thread = LoggerProvider::init();
    let logger = get_logger();

    if DEV { logger.warn("Running in Dev mode..."); }
    dotenv::dotenv().ok();

    let status = Arc::new(Mutex::new(true));
    set_exit_handler(&status);

    let (listener, poster) = adapters::napcat::get_pair();
    let adapter_status = listener.status.clone();
    let events = listener.events.clone();
    let adapter_thread = adapters::napcat::run_pair(listener, poster);

    let thinker = Thinker::init().await?;
    let thinker_status = thinker.status.clone();
    let (thinker_thread, think_end) = thinking::run(thinker);

    while *status.lock().unwrap() {
        if let Some(event) = events.lock().unwrap().pop_front() {
            match event {
                Event::Message(msg) => {
                    logger.chat(&format!("Msg: {} from {}", msg.raw, msg.sender.user_id));
                    if !run_cmds(msg.clone()).await {
                        let _ = think_end.send(msg);
                    }
                }
            }
        }
        sleep(Duration::from_secs_f32(CONFIG.heart_beat)).await;
    }

    logger.info("Exiting......");
    
    *adapter_status.lock().unwrap() = false;
    *thinker_status.lock().unwrap() = false;

    adapter_thread.await?;
    thinker_thread.await?;

    drop(logger);
    LoggerProvider::exit();
    logger_thread.await?;

    Ok(())
}



#[cfg(test)]
mod tests {
    use super::*;
    use rust_mc_status::McClient;
    use rustaris_ds::memory::{MemoryService, Scope};
    use serde_json::Value;
    use tokio;

    #[tokio::test]
    async fn test_memory_service() -> anyhow::Result<()> {
        let logger_thread = LoggerProvider::init();
        
        // 初始化内存服务
        let mem_service = MemoryService::init().await?;
        
        // 测试创建记忆
        let scope = Scope::Group(114514);
        let content = "Falsw最喜欢的人是小一";
        mem_service.create(scope, content).await?;
        
        // 测试相似记忆检索
        let similar_memories = mem_service.similars(scope, content).await?;
        
        // 验证创建的记忆能被检索到
        assert!(!similar_memories.is_empty(), "应该找到至少一个相似的记忆");
        assert_eq!(similar_memories[0].content, content, "检索到的记忆内容应该匹配");
        
        // 测试更新记忆
        let updated_content = "Falsw最讨厌的人是小一";
        mem_service.merge(similar_memories[0].id, updated_content, 0.8).await?;
        
        // 验证记忆已被更新
        let updated_memories = mem_service.similars(scope, updated_content).await?;
        assert!(!updated_memories.is_empty(), "应该找到更新后的记忆");
        assert_eq!(updated_memories[0].content, updated_content, "更新后的记忆内容应该匹配");
        assert_eq!(updated_memories[0].confidence, 0.8, "置信度应该正确更新");
        
        // 测试删除记忆
        mem_service.delete(updated_memories[0].id).await?;
        
        // 验证记忆已被删除
        let deleted_memories = mem_service.similars(scope, updated_content).await?;
        assert!(deleted_memories.is_empty(), "删除后应该找不到记忆");
        
        LoggerProvider::exit();
        logger_thread.await?;
        
        Ok(())
    }

    #[tokio::test]
    async fn test_mcs() -> anyhow::Result<()> {
        let client = McClient::new().with_max_parallel(5).with_timeout(Duration::from_secs(5));
        let status = client.ping("alive.falsw.top", rust_mc_status::ServerEdition::Java).await?;

        println!("{}", serde_json::to_string(&status)?);
        Ok(())
    }

    #[tokio::test]
    async fn test_netease() -> anyhow::Result<()> {
        let link = "http://192.168.3.38:8099/info".to_string();
        let client = reqwest::Client::new();

        let info = client.post(link)
            .json(&serde_json::json!({
                "id": 114514
            })).send().await?.json::<Value>().await?;
        
        println!("{:?}", info);

        Ok(())
    }
}


#[cfg(test)]
mod memory_tests {
    use std::{collections::HashMap, sync::{Arc, Mutex}};
    use tokio::{time::{sleep, Duration}};
    use rustaris_ds::{
        POSTER, SELFID, adapters::{APIRequest, APIWrapper}, logging::LoggerProvider, memory::{Dozer, MemoryService, Scope}, objects::{Group, Message, MessageArrayItem, Permission, User}, thinking::{AliasesMapping, Thinker}, tools::ToolRegistry
    };
    use deepseek_api::DeepSeekClientBuilder;

    #[tokio::test]
    async fn mem_test() -> anyhow::Result<()> {

        let logger_thread = LoggerProvider::init();

        dotenv::dotenv().ok();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<APIRequest>();
        POSTER.lock().unwrap().replace(APIWrapper { sender: tx });
        SELFID.lock().unwrap().replace(0);

        test_ai_memory_confidence_management().await?;
        //test_ai_memory_storage_and_retrieval().await?;
        //test_ai_memory_summary_and_extraction().await?;
        //test_long_term_memory_consistency().await?;
        //test_memory_recall_accuracy().await?;
        //test_memory_tool_interactions().await?;

        LoggerProvider::exit();
        logger_thread.await?;

        Ok(())
    }

    // 创建测试用的消息
    fn create_test_message(content: &str, user_id: usize, group_id: Option<usize>) -> Message {
        Message {
            raw: content.to_string(),
            sender: User {
                user_id,
                nickname: Some(format!("User{}", user_id)),
                card: Some(format!("Card{}", user_id)),
                role: Permission::Normal
            },
            private: group_id.is_none(),
            group: group_id.map(|id| Group {
                group_id: id,
                group_name: None
            }),
            message_id: 0,
            array: vec![MessageArrayItem::Text(content.to_string())],
        }
    }

    // 创建测试用的 Thinker 实例
    async fn create_test_thinker() -> anyhow::Result<Thinker> {

        // 初始化内存服务
        let mem_service = Arc::new(MemoryService::init().await?);

        let mut tools = ToolRegistry::new();
        // 注册记忆相关的工具
        tools.register(rustaris_ds::tools::AddMemoryTool { service: mem_service.clone() });
        tools.register(rustaris_ds::tools::UpdateMemoryTool { service: mem_service.clone() });
        tools.register(rustaris_ds::tools::DeleteMemoryTool { service: mem_service.clone() });

        let alia_map = Arc::new(Mutex::new(AliasesMapping::new()));

        Ok(Thinker {
            client: DeepSeekClientBuilder::new(std::env::var("API_KEY")?)
                .build()?,
            tools,
            channels: HashMap::new(),
            dozer: Dozer::new(mem_service, alia_map.clone()),
            status: Arc::new(Mutex::new(true)),
            alia_map: alia_map
        })
    }

    async fn test_ai_memory_storage_and_retrieval() -> anyhow::Result<()> {
        println!("=== 开始 AI 记忆存储和检索测试 ===");

        let mut thinker = create_test_thinker().await?;
        let mem_service = &thinker.dozer.mem_service.clone();

        // 场景1: 用户介绍个人信息
        let introduction_msg = create_test_message(
            "大家好，我是张三，我在北京工作，是一名软件工程师，我喜欢编程和打篮球，我的邮箱是 zhangsan@example.com", 
            1001, 
            Some(2001)
        );
        
        println!("发送消息: {}", introduction_msg.raw);
        
        // 处理消息，触发记忆提取
        thinker.resolve(introduction_msg).await?;
        
        // 等待记忆处理
        thinker.doze().await?;

        // 查询记忆
        let scope = Scope::Group(2001);
        let memories = mem_service.similars(scope, "张三").await?;
        
        println!("\n--- 检索到关于张三的记忆 ---");
        for memory in &memories {
            println!("  ID: {}, 内容: '{}', 置信度: {:.2}", 
                    memory.id, memory.content, memory.confidence);
        }

        // 场景2: 询问之前提到的信息
        let memories = mem_service.similars(scope, "有人知道张三的职业和爱好吗？").await?;
        
        println!("\n--- 直接查找：有人知道张三的职业和爱好吗？ ---");
        for memory in &memories {
            println!("  ID: {}, 内容: '{}', 置信度: {:.2}", 
                    memory.id, memory.content, memory.confidence);
        }

        // 场景3: 更新用户信息
        let update_msg = create_test_message(
            "不好意思，我想补充一下，我现在在上海工作了，不是北京", 
            1001, 
            Some(2001)
        );
        
        println!("\n发送更新消息: {}", update_msg.raw);
        
        // 处理更新消息
        thinker.resolve(update_msg).await?;
        
        thinker.doze().await?;

        // 检查更新后的记忆
        let updated_memories = mem_service.similars(scope, "张三 工作地点").await?;
        
        println!("\n--- 总结查找：张三 工作地点 ---");
        for memory in &updated_memories {
            println!("  内容: '{}', 置信度: {:.2}", memory.content, memory.confidence);
        }

        thinker.alia_map.lock().unwrap().save();

        println!("\n=== AI 记忆存储和检索测试完成 ===");
        Ok(())
    }

    async fn test_ai_memory_summary_and_extraction() -> anyhow::Result<()> {
        println!("=== 开始 AI 记忆总结和提取测试 ===");

        let mut thinker = create_test_thinker().await?;
        let mem_service = &thinker.dozer.mem_service.clone();

        // 模拟一个较长的对话序列，测试AI的信息提取能力
        let conversation = vec![
            create_test_message("我叫李四，是一名产品经理", 1004, Some(2002)),
            create_test_message("我负责过多个移动应用项目", 1004, Some(2002)),
            create_test_message("我对用户体验设计很有研究", 1004, Some(2002)),
            create_test_message("我住在杭州，喜欢周末爬山", 1004, Some(2002)),
            create_test_message("我的联系方式是 lisi@company.com", 1004, Some(2002)),
            create_test_message("另外我还会弹吉他，有空的时候喜欢演奏", 1004, Some(2002)),
        ];

        println!("\n--- 发送多轮对话以供AI总结 ---");
        for (i, msg) in conversation.iter().enumerate() {
            println!("第{}轮: {}", i + 1, msg.raw);
            thinker.resolve(msg.clone()).await?;
            sleep(Duration::from_millis(800)).await;
        }

        // 等待Dozer处理积累的消息
        thinker.doze().await?;

        // 查询AI提取的关键信息
        let scope = Scope::Group(2002);
        let all_memories = mem_service.similars(scope, "李四").await?;
        
        println!("\n--- AI 提取出的关于李四的记忆 ---");
        for memory in &all_memories {
            println!("  ID: {}, 内容: '{}', 置信度: {:.2}", 
                    memory.id, memory.content, memory.confidence);
        }

        // 测试记忆的关联查询
        let location_memories = mem_service.similars(scope, "杭州").await?;
        println!("\n--- 关于杭州的记忆 ---");
        for memory in &location_memories {
            println!("  内容: '{}', 置信度: {:.2}", memory.content, memory.confidence);
        }

        // 测试兴趣爱好的记忆
        let hobby_memories = mem_service.similars(scope, "吉他").await?;
        println!("\n--- 关于吉他的记忆 ---");
        for memory in &hobby_memories {
            println!("  内容: '{}', 置信度: {:.2}", memory.content, memory.confidence);
        }

        thinker.alia_map.lock().unwrap().save();

        println!("\n=== AI 记忆总结和提取测试完成 ===");
        Ok(())
    }

    async fn test_ai_memory_confidence_management() -> anyhow::Result<()> {
        println!("=== 开始 AI 记忆置信度管理测试 ===");

        let mut thinker = create_test_thinker().await?;
        let mem_service = &thinker.dozer.mem_service.clone();

        // 初始信息
        let initial_info = create_test_message(
            "我的专业是计算机科学，我精通Java和Python", 
            1005, 
            Some(2003)
        );
        
        println!("初始信息: {}", initial_info.raw);
        thinker.resolve(initial_info).await?;
        thinker.doze().await?;

        let initial_memories = mem_service.similars(Scope::Group(2003), "Java Python").await?;
        println!("\n--- 包含Java和Python的初始记忆 ---");
        for memory in &initial_memories {
            println!("  内容: '{}', 置信度: {:.2}", memory.content, memory.confidence);
        }

        // 支持性信息（增加置信度）
        let supporting_info = create_test_message(
            "是的，我经常用Java做后端开发，用Python做数据分析", 
            1005, 
            Some(2003)
        );
        
        println!("\n支持性信息: {}", supporting_info.raw);
        thinker.resolve(supporting_info).await?;
        thinker.doze().await?;

        // 检查初始记忆的置信度变化
        let initial_memories = mem_service.similars(Scope::Group(2003), "Java Python").await?;
        println!("\n--- 包含Java和Python的强化记忆 ---");
        for memory in &initial_memories {
            println!("  内容: '{}', 置信度: {:.2}", memory.content, memory.confidence);
        }

        // 冲突信息（降低置信度并修正）
        let conflicting_info = create_test_message(
            "等等，我要纠正一下，我主要用JavaScript和Go，不是Java和Python", 
            1005, 
            Some(2003)
        );
        
        println!("\n冲突信息: {}", conflicting_info.raw);
        thinker.resolve(conflicting_info).await?;
        thinker.doze().await?;

        // 检查修正后的记忆
        let corrected_memories = mem_service.similars(Scope::Group(2003), "JavaScript Go").await?;
        println!("\n--- 修正后关于JavaScript和Go的记忆 ---");
        for memory in &corrected_memories {
            println!("  内容: '{}', 置信度: {:.2}", memory.content, memory.confidence);
        }

        thinker.alia_map.lock().unwrap().save();

        println!("\n=== AI 记忆置信度管理测试完成 ===");
        Ok(())
    }

    async fn test_memory_tool_interactions() -> anyhow::Result<()> {
        println!("=== 开始 记忆工具交互测试 ===");

        let mut thinker = create_test_thinker().await?;
        let mem_service = &thinker.dozer.mem_service.clone();

        // 场景：测试AI如何使用各种记忆工具
        let detailed_info = create_test_message(
            "我有一个朋友叫王五，他在一家互联网公司担任架构师，他最擅长的技术栈是微服务架构和容器化部署", 
            1007, 
            Some(2004)
        );
        
        println!("发送详细信息: {}", detailed_info.raw);
        thinker.resolve(detailed_info).await?;
        thinker.doze().await?;

        // 查询相关信息
        let query = create_test_message(
            "有没有人提到过王五？他是什么职业？", 
            1008, 
            Some(2004)
        );
        
        println!("发送查询: {}", query.raw);
        thinker.resolve(query).await?;
        thinker.doze().await?;

        // 检查提取的记忆
        let scope = Scope::Group(2004);
        let memories = mem_service.similars(scope, "王五 架构师").await?;
        
        println!("\n--- 提取到关于王五的记忆 ---");
        for memory in &memories {
            println!("  内容: '{}', 置信度: {:.2}", memory.content, memory.confidence);
        }

        // 测试删除功能（通过发送否定信息）
        let deletion_trigger = create_test_message(
            "实际上，王五不是架构师，他是项目经理", 
            1007, 
            Some(2004)
        );
        
        println!("\n发送修正信息: {}", deletion_trigger.raw);
        thinker.resolve(deletion_trigger).await?;
        thinker.doze().await?;

        // 验证记忆更新
        let updated_memories = mem_service.similars(scope, "王五").await?;
        println!("\n--- 更新后的王五相关信息 ---");
        for memory in &updated_memories {
            println!("  内容: '{}', 置信度: {:.2}", memory.content, memory.confidence);
        }

        thinker.alia_map.lock().unwrap().save();

        println!("\n=== 记忆工具交互测试完成 ===");
        Ok(())
    }

    async fn test_long_term_memory_consistency() -> anyhow::Result<()> {
        println!("=== 开始 长期记忆一致性测试 ===");

        let mut thinker = create_test_thinker().await?;
        let mem_service = &thinker.dozer.mem_service.clone();

        // 模拟跨时间段的记忆测试
        let user_introductions = vec![
            create_test_message("我是赵六，我是一名数据分析师", 1009, Some(2005)),
            create_test_message("我主要使用Python和SQL进行数据分析", 1009, Some(2005)),
            create_test_message("我也对机器学习很感兴趣", 1009, Some(2005)),
        ];

        println!("\n--- 第一轮对话：用户自我介绍 ---");
        for msg in &user_introductions {
            println!("发送: {}", msg.raw);
            thinker.resolve(msg.clone()).await?;
            sleep(Duration::from_millis(500)).await;
        }

        // 等待Dozer处理
        thinker.doze().await?;

        // 检查第一轮记忆
        let first_memories = mem_service.similars(Scope::Group(2005), "赵六").await?;
        println!("\n--- 第一轮后关于赵六的记忆 ---");
        for (i, memory) in first_memories.iter().enumerate() {
            println!("  {}: 内容: '{}', 置信度: {:.2}", i+1, memory.content, memory.confidence);
        }

        // 添加更多关于同一用户的信息
        let additional_info = vec![
            create_test_message("我最近在学习深度学习技术", 1009, Some(2005)),
            create_test_message("我的工作主要涉及金融行业的数据分析", 1009, Some(2005)),
        ];

        println!("\n--- 第二轮对话：补充信息 ---");
        for msg in &additional_info {
            println!("发送: {}", msg.raw);
            thinker.resolve(msg.clone()).await?;
            sleep(Duration::from_millis(500)).await;
        }

        thinker.doze().await?;

        // 检查整合后的记忆
        let consolidated_memories = mem_service.similars(Scope::Group(2005), "赵六").await?;
        println!("\n--- 整合后关于赵六的记忆 ---");
        for (i, memory) in consolidated_memories.iter().enumerate() {
            println!("  {}: 内容: '{}', 置信度: {:.2}", i+1, memory.content, memory.confidence);
        }
        
        let consolidated_memories = mem_service.similars(Scope::Group(2005), "谁能告诉我赵六是做什么的？他都掌握哪些技术？").await?;
        for (i, memory) in consolidated_memories.iter().enumerate() {
            println!("  {}: 内容: '{}', 置信度: {:.2}", i+1, memory.content, memory.confidence);
        }

        thinker.alia_map.lock().unwrap().save();

        println!("\n=== 长期记忆一致性测试完成 ===");
        Ok(())
    }

    async fn test_memory_recall_accuracy() -> anyhow::Result<()> {
        println!("=== 开始 记忆召回准确度测试 ===");

        let mut thinker = create_test_thinker().await?;
        let mem_service = &thinker.dozer.mem_service.clone();

        // 创建多个不同的用户信息
        let users_info = vec![
            create_test_message("我是孙七，软件工程师，熟悉Java和Spring框架", 1011, Some(2006)),
            create_test_message("我是周八，UI设计师，擅长Figma和Sketch", 1012, Some(2006)),
            create_test_message("我是吴九，运维工程师，专精Docker和Kubernetes", 1013, Some(2006)),
        ];

        println!("\n--- 输入多个用户的详细信息 ---");
        for msg in &users_info {
            println!("输入: {}", msg.raw);
            thinker.resolve(msg.clone()).await?;
            sleep(Duration::from_millis(600)).await;
        }

        thinker.doze().await?;

        // 测试精确召回
        let queries = vec![
            ("谁能告诉我孙七的职业？", "孙七"),
            ("周八的专业技能是什么？", "周八"),
            ("吴九擅长哪些技术？", "吴九"),
            ("谁熟悉Spring框架？", "Spring"),
            ("谁擅长Figma？", "Figma"),
        ];

        for (query_str, keyword) in &queries {
            let db_results = mem_service.similars(Scope::Group(2006), *query_str).await?;
            println!("  数据库召回的相关记忆: {}", query_str);
            for memory in &db_results {
                println!("    - '{}'", memory.content);
            }
            let db_results = mem_service.similars(Scope::Group(2006), keyword).await?;
            println!("  数据库召回的相关记忆: {}", keyword);
            for memory in &db_results {
                println!("    - '{}'", memory.content);
            }
        }

        thinker.alia_map.lock().unwrap().save();

        println!("\n=== 记忆召回准确度测试完成 ===");
        Ok(())
    }
}