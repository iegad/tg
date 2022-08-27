use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use tg::{g, nw::pack};

#[async_trait]
trait Handler: Send + Sync + Clone + Copy + Sized {
    fn get_request_id(&self) -> u16;
    fn get_response_id(&self) -> u16;
    fn remark(&self) -> &str;
    async fn handle(&self, req: &pack::Package) -> pack::Package;
}



#[tokio::main]
async fn main() -> g::Result<()> {
    let map = HashMap::new();

    Ok(())
}
