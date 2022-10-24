use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from("./src/pb");
    if let Err(err) = tonic_build::configure().file_descriptor_set_path(out_dir).compile(&["src/proto/warsong.proto"], &[""]) {
        panic!(">>>>>>>>>>> {err}");
    }
}
