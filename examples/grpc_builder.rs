#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::compile_protos("src/pb/lisa.proto")?;
    Ok(())
}
