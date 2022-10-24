fn main() {
    if let Err(err) = tonic_build::configure().out_dir("src/proto").compile(&["warsong.proto"], &["src/proto"]) {
        panic!(">>>>>>>>>>> {err}");
    }
}
