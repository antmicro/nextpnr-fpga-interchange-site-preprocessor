use capnpc;
use std::path::Path;
use std::env;

fn compile_capnp(path: &Path, prefix: &Path) {
    if !path.is_file() {
        panic!("File \"{}\" does not exist!", path.to_str().unwrap());
    }
    println!("Compiling schema {}", path.file_name().unwrap().to_str().unwrap());
    capnpc::CompilerCommand::new()
        .file(path)
        .default_parent_module(vec!["ic_loader".into()])
        .src_prefix(prefix)
        .run()
        .expect("Compiling schema");
}

fn main() {
    let schema_path = env::var("FPGA_INTERCHANGE_SCHEMA_DIR")
        .unwrap_or("third_party/fpga-interchange-schema".to_string());
    let schema_path = Path::new(&schema_path);

    println!("fpga-interchange schema directory: {}", schema_path.to_str().unwrap());
    compile_capnp(&schema_path.join("interchange/DeviceResources.capnp"), &schema_path);
    compile_capnp(&schema_path.join("interchange/LogicalNetlist.capnp"), &schema_path);
    compile_capnp(&schema_path.join("interchange/PhysicalNetlist.capnp"), &schema_path);
    compile_capnp(&schema_path.join("interchange/References.capnp"), &schema_path);
}
