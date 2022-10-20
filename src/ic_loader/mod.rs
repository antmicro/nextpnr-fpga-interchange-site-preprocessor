/* Copyright (C) 2022 Antmicro
 * 
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 * 
 *     https://www.apache.org/licenses/LICENSE-2.0
 * 
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */


#[allow(non_snake_case, warnings)]
pub mod References_capnp {
    include_interchange_capnp!("References_capnp.rs");
}

#[allow(non_snake_case, warnings)]
pub mod DeviceResources_capnp {
    include_interchange_capnp!("DeviceResources_capnp.rs");
}

#[allow(non_snake_case, warnings)]
pub mod LogicalNetlist_capnp {
    include_interchange_capnp!("LogicalNetlist_capnp.rs");
}

#[allow(non_snake_case, warnings)]
pub mod PhysicalNetlist_capnp {
    include_interchange_capnp!("PhysicalNetlist_capnp.rs");
}

use std::path::Path;
use std::fs::File;
use std::io::BufReader;
use memmap2::Mmap;
use flate2::read::GzDecoder;

#[derive(Debug, Clone)]
pub enum OpenWriteError {
    CantOpenFile(String),
    CapnProtoError(String)
}

const CPNP_MSG_MAXSIZE: usize = usize::MAX; // 4GiB

pub struct OpenOpts {
    pub raw: bool,
}

pub struct WriteOpts {
    pub raw: bool,
    pub compresion_level: u32
}

impl Default for OpenOpts {
    fn default() -> Self {
        Self {
            raw: false
        }
    }
}

pub trait MsgReader {
    /* This is dumb, but GATs are STILL unstable (seriously???) */
    fn get_archdef_root<'a>(&'a self) -> Result<archdef::Root<'a>, capnp::Error>;
    fn get_logical_netlist_root<'a>(&'a self) -> Result<logical_netlist::Root<'a>, capnp::Error>;
}

impl<S> MsgReader for capnp::message::Reader<S> where
    S: capnp::message::ReaderSegments
{
    fn get_archdef_root<'a>(&'a self) -> Result<archdef::Root<'a>, capnp::Error> {
        self.get_root::<archdef::Root<'a>>()
    }

    fn get_logical_netlist_root<'a>(&'a self) -> Result<logical_netlist::Root<'a>, capnp::Error> {
        self.get_root::<logical_netlist::Root<'a>>()
    }
}

pub fn open<P>(path: P, opts: OpenOpts) -> Result<Box<dyn MsgReader>, OpenWriteError> where
    P: AsRef<Path>,

{
    let archdef_file = File::open(path)
        .map_err(|e| OpenWriteError::CantOpenFile(format!("{:?}", e)))?;
    
    let reader_opts = capnp::message::ReaderOptions {
        traversal_limit_in_words: Some(CPNP_MSG_MAXSIZE),
        .. capnp::message::DEFAULT_READER_OPTIONS
    };
    
    /* RAW mode uses memory mapping and is highly recommended over GZIP for debug builds
     * due to much faster load times.
     * For realease builds, loading a gzipped file doesn't seem to take noticeably longer
     * than using memory-mapped files. 
     * 
     * IMPORTANT: In order to use RAW mode, you must decompress the fpga-interchange
     * device file using gzip.
     */
    let reader: Box<dyn MsgReader> = if opts.raw {
        /* UNSAFE DUE TO A POTENTIAL UB WHEN A FILE IS CHANGED! */
        let mmapped = unsafe { Mmap::map(&archdef_file) }
            .map_err(|e| OpenWriteError::CantOpenFile(format!("mmap failed: {:?}", e)))?;
        let segments = capnp::serialize::BufferSegments::new(mmapped, reader_opts)
            .map_err(|e| OpenWriteError::CapnProtoError(format!("failed to create buffer segments: {:?}", e)))?;
        Box::new(capnp::message::Reader::new(segments, reader_opts))
    } else {
        let d = BufReader::new(GzDecoder::new(archdef_file));
    
        let reader = capnp::serialize::read_message(
            d,
            capnp::message::ReaderOptions {
                traversal_limit_in_words: Some(CPNP_MSG_MAXSIZE),
                .. capnp::message::DEFAULT_READER_OPTIONS
            }
        ).map_err(|e| OpenWriteError::CapnProtoError(format!("{:?}", e)))?;
        Box::new(reader)
    };
    
    Ok(reader)
}

pub mod archdef;
pub mod logical_netlist;
