//! Reticulum Mesh Network Stack integration module.

pub mod address;
pub mod auto_interface;
pub mod interface;
pub mod link;
pub mod packet;
pub mod routing;
pub mod slip;
pub mod tcp_interface;
pub mod transport;

pub use address::ReticulumAddress;
pub use auto_interface::{AutoInterface, AutoInterfaceConfig};
pub use interface::{ReticulumInterfaceKind, ReticulumInterfaceStatus};
pub use link::{LinkInfo, LinkManager, LinkState};
pub use packet::{ReticulumPacket, ReticulumPacketType};
pub use routing::{PathEntry, PathTable};
pub use slip::{slip_decode, slip_encode};
pub use tcp_interface::{TcpClientInterface, TcpInterfaceConfig, TcpServerInterface};
pub use transport::{node_for, ReticulumNode, ReticulumNodeStatus};
