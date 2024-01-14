#[tokio::main]
async fn main() {
    match pso2ship_server::run().await {
        Ok(_) => {}
        Err(e) => eprintln!("Server error: {e}"),
    }
}
