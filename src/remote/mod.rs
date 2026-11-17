pub mod client;
pub mod discovery;
pub mod negotiation;
pub mod packfile;
pub mod pkt_line;
pub mod sideband;
pub mod types;

pub use client::GitHttpClient;
pub use negotiation::{build_v1_fetch_request, build_v2_fetch_request};
pub use packfile::unpack_packfile_into;
pub use pkt_line::{PktLine, decode_pkt_lines, read_pkt_line, write_pkt_line};
pub use sideband::{extract_packfile_bytes, extract_packfile_stream};
pub use types::{FetchResult, RefAdvertisement, RemoteError, RemoteRef, ServerCapabilities};
