use crate::objects::Message;



pub async fn run_cmds(msg: Message) -> bool {

    let mut flag = false;

    if msg.on_command("#echo") {
        msg.quick_send(&msg.joint_args()).await;
        flag = true;
    }

    flag
}