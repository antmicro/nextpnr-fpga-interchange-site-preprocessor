use std::fs::File;
use std::path::Path;
use capnp;
use std::io::BufWriter;
use flate2;
use flate2::Compression;

use flate2::write::GzEncoder;

use super::*;

pub type DeviceBuilder = capnp::message::TypedBuilder::<
    DeviceResources_capnp::device::Owned
>;
pub type Root<'a> = DeviceResources_capnp::device::Reader<'a>;
//type RootBuilder<'a> = archdef::DeviceResources_capnp::device::Builder<'a>;
pub type TileReader<'a> = DeviceResources_capnp::device::tile::Reader<'a>;
pub type TileTypeReader<'a> = DeviceResources_capnp::device::tile_type::Reader<'a>;
/* type WiresReader<'a> = capnp::struct_list::Reader<
    'a, 
    crate::archdef::DeviceResources_capnp::device::wire::Owned
>; */

pub type WireReader<'a> = DeviceResources_capnp::device::wire::Reader<'a>;

pub fn make_builder<'a>(root: DeviceResources_capnp::device::Reader<'a>) -> DeviceBuilder {
    let mut builder = DeviceBuilder::new_default();
    builder.set_root(root.clone()).unwrap();
    builder
}

pub fn write<P>(path: P, builder: DeviceBuilder, opts: WriteOpts)
    -> Result<(), OpenWriteError> where P: AsRef<Path>
{
    let archdef_file = File::create(path)
        .map_err(|e| OpenWriteError::CantOpenFile(format!("{:?}", e)))?;
    
    if opts.raw {
        capnp::serialize::write_message(archdef_file, &builder.into_inner())
            .map_err(|e| OpenWriteError::CapnProtoError(format!("failed to write arch, {:?}", e)))?;
    } else {
        let e = BufWriter::new(GzEncoder::new(archdef_file, Compression::new(opts.compresion_level)));
        capnp::serialize::write_message(e, &builder.into_inner())
            .map_err(|e| OpenWriteError::CapnProtoError(format!("failed to write arch, {:?}", e)))?;
        }

    Ok(())
}
