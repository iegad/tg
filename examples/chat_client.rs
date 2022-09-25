use tg::utils;

#[tokio::main]
async fn main() {
    utils::init_log(tracing::Level::DEBUG);

    let std_in = std::io::stdin();
    let mut line = String::new();

    while let Ok(_) = std_in.read_line(&mut line) {
        let tmp = line.trim();

        if tmp.to_lowercase() == "exit" {
            tracing::debug!("EXIT.");
            break;
        }
    }
}
