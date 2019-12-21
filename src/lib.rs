#![deny(clippy::all)]

//! Socketcan ISO-TP support.
//!
//! The Linux kernel supports using CAN-devices through a
//! [network-like API](https://www.kernel.org/doc/Documentation/networking/can.txt).
//! This crate allows easy access to this functionality without having to wrestle
//! libc calls.
//!
//! ISO-TP allows sending data packets that exceed the eight byte of a default CAN frame.
//! [can-isotp](https://github.com/hartkopp/can-isotp) is an ISO-TP kernel module that takes
//! care of handling the ISO-TP protocol.
//!
//! Instructions on how the can-isotp kernel module can be build and loaded can be found
//! at [https://github.com/hartkopp/can-isotp](https://github.com/hartkopp/can-isotp) .
//!
//! ```rust,no_run
//! use socketcan_isotp::IsoTpSocket;
//!
//! fn main() -> Result<(), socketcan_isotp::Error> {
//!     let mut tp_socket = IsoTpSocket::open(
//!         "vcan0",
//!         0x123,
//!         0x321
//!     )?;
//!
//!     loop {
//!         let buffer = tp_socket.read()?;
//!         println!("read {} bytes", buffer.len());
//!
//!         // print TP frame data
//!         for x in buffer {
//!             print!("{:X?} ", x);
//!         }
//!
//!         println!("");
//!     }
//!
//!     Ok(())
//! }
//! ```
//!

use bitflags::bitflags;
use libc::{
    bind, c_int, c_short, c_void, close, fcntl, read, setsockopt, sockaddr, socket, write, F_GETFL,
    F_SETFL, O_NONBLOCK, SOCK_DGRAM,
};
use nix::net::if_::if_nametoindex;
use std::convert::TryFrom;
use std::convert::TryInto;
use std::io;
use std::mem::size_of;
use std::num::TryFromIntError;
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
use std::time::Duration;
use thiserror::Error;

/// CAN address family
pub const AF_CAN: c_short = 29;

/// CAN protocol family
pub const PF_CAN: c_int = 29;

/// ISO 15765-2 Transport Protocol
pub const CAN_ISOTP: c_int = 6;

/// undocumented can.h constant
pub const SOL_CAN_BASE: c_int = 100;

/// undocumented isotp.h constant
pub const SOL_CAN_ISOTP: c_int = SOL_CAN_BASE + CAN_ISOTP;

/// pass struct `IsoTpOptions`
pub const CAN_ISOTP_OPTS: c_int = 1;

/// pass struct `FlowControlOptions`
pub const CAN_ISOTP_RECV_FC: c_int = 2;

/// pass __u32 value in nano secs
/// use this time instead of value
/// provided in FC from the receiver
pub const CAN_ISOTP_TX_STMIN: c_int = 3;

/// pass __u32 value in nano secs
/// ignore received CF frames which
/// timestamps differ less than val
pub const CAN_ISOTP_RX_STMIN: c_int = 4;

/// pass struct `LinkLayerOptions`
pub const CAN_ISOTP_LL_OPTS: c_int = 5;

/// `CAN_MAX_DLEN` According to ISO 11898-1
pub const CAN_MAX_DLEN: u8 = 8;

/// Size of buffer allocated for reading TP data
const RECV_BUFFER_SIZE: usize = 4096;

/// Size of a canframe, constant to reduce crate dependencies
/// `std::mem::size_of::<socketcan::CANFrame>())`
const SIZE_OF_CAN_FRAME: u8 = 16;

const FLOW_CONTROL_OPTIONS_SIZE: usize = size_of::<FlowControlOptions>();

const ISOTP_OPTIONS_SIZE: usize = size_of::<IsoTpOptions>();

const LINK_LAYER_OPTIONS_SIZE: usize = size_of::<LinkLayerOptions>();

bitflags! {
    pub struct IsoTpBehaviour: u32 {
        /// listen only (do not send FC)
        const CAN_ISOTP_LISTEN_MODE = 0x001;
        /// enable extended addressing
        const CAN_ISOTP_EXTEND_ADDR	= 0x002;
        /// enable CAN frame padding tx path
        const CAN_ISOTP_TX_PADDING	= 0x004;
        /// enable CAN frame padding rx path
        const CAN_ISOTP_RX_PADDING	= 0x008;
        /// check received CAN frame padding
        const CAN_ISOTP_CHK_PAD_LEN	= 0x010;
        /// check received CAN frame padding
        const CAN_ISOTP_CHK_PAD_DATA = 0x020;
        /// half duplex error state handling
        const CAN_ISOTP_HALF_DUPLEX = 0x040;
        /// ignore stmin from received FC
        const CAN_ISOTP_FORCE_TXSTMIN = 0x080;
        /// ignore CFs depending on rx stmin
        const CAN_ISOTP_FORCE_RXSTMIN = 0x100;
        /// different rx extended addressing
        const CAN_ISOTP_RX_EXT_ADDR = 0x200;
    }
}

/// if set, indicate 29 bit extended format
pub const EFF_FLAG: u32 = 0x8000_0000;

/// remote transmission request flag
pub const RTR_FLAG: u32 = 0x4000_0000;

/// error flag
pub const ERR_FLAG: u32 = 0x2000_0000;

/// valid bits in standard frame id
pub const SFF_MASK: u32 = 0x0000_07ff;

/// valid bits in extended frame id
pub const EFF_MASK: u32 = 0x1fff_ffff;

/// valid bits in error frame
pub const ERR_MASK: u32 = 0x1fff_ffff;

/// an error mask that will cause Socketcan to report all errors
pub const ERR_MASK_ALL: u32 = ERR_MASK;

/// an error mask that will cause Socketcan to silently drop all errors
pub const ERR_MASK_NONE: u32 = 0;

#[derive(Debug)]
#[repr(C)]
struct CanAddr {
    _af_can: c_short,
    if_index: c_int, // address familiy,
    /// transport protocol class address information
    rx_id: u32,
    /// transport protocol class address information
    tx_id: u32,
    _pgn: u32,
    _addr: u8,
}

/// ISO-TP otions aka `can_isotp_options`
#[repr(C)]
pub struct IsoTpOptions {
    /// set flags for isotp behaviour.
    flags: u32,
    /// frame transmission time (N_As/N_Ar)
    /// time in nano secs
    frame_txtime: u32,
    /// set address for extended addressing
    ext_address: u8,
    /// set content of padding byte (tx)
    txpad_content: u8,
    /// set content of padding byte (rx)
    rxpad_content: u8,
    /// set address for extended addressing
    rx_ext_address: u8,
}

impl IsoTpOptions {
    pub fn new(
        flags: IsoTpBehaviour,
        frame_txtime: Duration,
        ext_address: u8,
        txpad_content: u8,
        rxpad_content: u8,
        rx_ext_address: u8,
    ) -> Result<Self, TryFromIntError> {
        let flags = flags.bits();
        let frame_txtime = u32::try_from(frame_txtime.as_nanos())?;

        Ok(Self {
            flags,
            frame_txtime,
            ext_address,
            txpad_content,
            rxpad_content,
            rx_ext_address,
        })
    }

    /// get flags for isotp behaviour.
    pub fn get_flags(&self) -> Option<IsoTpBehaviour> {
        IsoTpBehaviour::from_bits(self.flags)
    }

    /// set flags for isotp behaviour.
    pub fn set_flags(&mut self, flags: IsoTpBehaviour) {
        self.flags = flags.bits();
    }

    /// get frame transmission time (N_As/N_Ar)
    pub fn get_frame_txtime(&self) -> Duration {
        Duration::from_nanos(self.frame_txtime.into())
    }

    /// set frame transmission time (N_As/N_Ar)
    pub fn set_frame_txtime(&mut self, frame_txtime: Duration) -> Result<(), TryFromIntError> {
        self.frame_txtime = u32::try_from(frame_txtime.as_nanos())?;
        Ok(())
    }

    /// get frame transmission time (N_As/N_Ar)
    pub fn get_ext_address(&self) -> u8 {
        self.ext_address
    }

    /// set address for extended addressing
    pub fn set_ext_address(&mut self, ext_address: u8) {
        self.ext_address = ext_address;
    }

    /// get address for extended addressing
    pub fn get_txpad_content(&self) -> u8 {
        self.txpad_content
    }

    /// set content of padding byte (tx)
    pub fn set_txpad_content(&mut self, txpad_content: u8) {
        self.txpad_content = txpad_content;
    }

    /// get content of padding byte (rx)
    pub fn get_rxpad_content(&self) -> u8 {
        self.rxpad_content
    }

    /// set content of padding byte (rx)
    pub fn set_rxpad_content(&mut self, rxpad_content: u8) {
        self.rxpad_content = rxpad_content;
    }

    /// get address for extended addressing
    pub fn get_rx_ext_address(&self) -> u8 {
        self.rx_ext_address
    }

    /// set address for extended addressing
    pub fn set_rx_ext_address(&mut self, rx_ext_address: u8) {
        self.rx_ext_address = rx_ext_address;
    }
}

impl Default for IsoTpOptions {
    fn default() -> Self {
        // Defaults defined in linux/can/isotp.h
        Self {
            flags: 0x00,
            frame_txtime: 0x00,
            ext_address: 0x00,
            txpad_content: 0xCC,
            rxpad_content: 0xCC,
            rx_ext_address: 0x00,
        }
    }
}

/// Flow control options aka `can_isotp_fc_options`
#[repr(C)]
pub struct FlowControlOptions {
    /// blocksize provided in FC frame
    /// 0 = off
    bs: u8,
    /// separation time provided in FC frame
    ///
    /// 0x00 - 0x7F : 0 - 127 ms
    /// 0x80 - 0xF0 : reserved
    /// 0xF1 - 0xF9 : 100 us - 900 us
    /// 0xFA - 0xFF : reserved
    stmin: u8,
    /// max. number of wait frame transmiss.
    /// 0 = omit FC N_PDU WT
    wftmax: u8,
}

impl Default for FlowControlOptions {
    fn default() -> Self {
        Self {
            // CAN_ISOTP_DEFAULT_RECV_BS
            bs: 0,
            // CAN_ISOTP_DEFAULT_RECV_STMIN
            stmin: 0x00,
            // CAN_ISOTP_DEFAULT_RECV_WFTMAX
            wftmax: 0,
        }
    }
}

bitflags! {
    pub struct TxFlags: u8 {
        /// bit rate switch (second bitrate for payload data)
        const CANFD_BRS = 0x01;
        /// error state indicator of the transmitting node
        const CANFD_ESI	= 0x02;
    }
}

/// Link layer options aka `can_isotp_ll_options`
#[repr(C)]
pub struct LinkLayerOptions {
    /// generated & accepted CAN frame type
    /// CAN_MTU   (16) -> standard CAN 2.0
    /// CANFD_MTU (72) -> CAN FD frame
    mtu: u8,
    /// tx link layer data length in bytes
    /// (configured maximum payload length)
    /// __u8 value : 8,12,16,20,24,32,48,64
    /// => rx path supports all LL_DL values
    tx_dl: u8,
    /// set into struct canfd_frame.flags	*/
    /// at frame creation: e.g. CANFD_BRS
    /// Obsolete when the BRS flag is fixed
    /// by the CAN netdriver configuration
    tx_flags: u8,
}

impl LinkLayerOptions {
    pub fn new(mtu: u8, tx_dl: u8, tx_flags: TxFlags) -> Self {
        let tx_flags = tx_flags.bits();
        Self {
            mtu,
            tx_dl,
            tx_flags,
        }
    }
}

impl Default for LinkLayerOptions {
    fn default() -> Self {
        Self {
            // CAN_ISOTP_DEFAULT_LL_MTU
            mtu: SIZE_OF_CAN_FRAME,
            // CAN_ISOTP_DEFAULT_LL_TX_DL
            tx_dl: CAN_MAX_DLEN,
            // CAN_ISOTP_DEFAULT_LL_TX_FLAGS
            tx_flags: 0x00,
        }
    }
}

#[derive(Error, Debug)]
/// Possible errors
pub enum Error {
    /// CAN device could not be found
    #[error("Failed to find can device: {source:?}")]
    LookupError {
        #[from]
        source: nix::Error,
    },

    /// IO Error
    #[error("IO error: {source:?}")]
    IOError {
        #[from]
        source: io::Error,
    },
}
/// An ISO-TP socketcan socket.
///
/// Will be closed upon deallocation. To close manually, use `std::drop::Drop`.
/// Internally this is just a wrapped file-descriptor.
pub struct IsoTpSocket {
    fd: c_int,
    recv_buffer: [u8; RECV_BUFFER_SIZE],
}

impl IsoTpSocket {
    /// Open a named CAN ISO-TP device.
    ///
    /// Usually the more common case, opens a socket can device by name, such
    /// as "vcan0" or "socan0".
    pub fn open(ifname: &str, src: u32, dst: u32) -> Result<Self, Error> {
        Self::open_with_opts(
            ifname,
            src,
            dst,
            Some(IsoTpOptions::default()),
            Some(FlowControlOptions::default()),
            Some(LinkLayerOptions::default()),
        )
    }

    /// Open a named CAN ISO-TP device, passing additional options.
    ///
    /// Usually the more common case, opens a socket can device by name, such
    /// as "vcan0" or "socan0".
    pub fn open_with_opts(
        ifname: &str,
        src: u32,
        dst: u32,
        isotp_options: Option<IsoTpOptions>,
        rx_flow_control_options: Option<FlowControlOptions>,
        link_layer_options: Option<LinkLayerOptions>,
    ) -> Result<Self, Error> {
        let if_index = if_nametoindex(ifname)?;
        Self::open_if_with_opts(
            if_index.try_into().unwrap(),
            src,
            dst,
            isotp_options,
            rx_flow_control_options,
            link_layer_options,
        )
    }

    /// Open CAN ISO-TP device device by interface number.
    ///
    /// Opens a CAN device by kernel interface number.
    pub fn open_if(if_index: c_int, src: u32, dst: u32) -> Result<Self, Error> {
        Self::open_if_with_opts(
            if_index.try_into().unwrap(),
            src,
            dst,
            Some(IsoTpOptions::default()),
            Some(FlowControlOptions::default()),
            Some(LinkLayerOptions::default()),
        )
    }

    /// Open CAN ISO-TP device device by interface number, passing additional options.
    ///
    /// Opens a CAN device by kernel interface number.
    pub fn open_if_with_opts(
        if_index: c_int,
        src: u32,
        dst: u32,
        isotp_options: Option<IsoTpOptions>,
        rx_flow_control_options: Option<FlowControlOptions>,
        link_layer_options: Option<LinkLayerOptions>,
    ) -> Result<Self, Error> {
        let addr = CanAddr {
            _af_can: AF_CAN,
            if_index,
            rx_id: if dst > 0x7FF { dst | EFF_FLAG } else { dst },
            tx_id: if src > 0x7FF { src | EFF_FLAG } else { src },
            _pgn: 0,
            _addr: 0,
        };

        // open socket
        let sock_fd;
        unsafe {
            sock_fd = socket(PF_CAN, SOCK_DGRAM, CAN_ISOTP);
        }

        if sock_fd == -1 {
            return Err(Error::from(io::Error::last_os_error()));
        }

        // Set IsoTpOptions
        if let Some(isotp_options) = isotp_options {
            let isotp_options_ptr: *const c_void = &isotp_options as *const _ as *const c_void;
            let err = unsafe {
                setsockopt(
                    sock_fd,
                    SOL_CAN_ISOTP,
                    CAN_ISOTP_OPTS,
                    isotp_options_ptr,
                    ISOTP_OPTIONS_SIZE.try_into().unwrap(),
                )
            };
            if err == -1 {
                return Err(Error::from(io::Error::last_os_error()));
            }
        }

        // Set FlowControlOptions
        if let Some(rx_flow_control_options) = rx_flow_control_options {
            let rx_flow_control_options_ptr: *const c_void =
                &rx_flow_control_options as *const _ as *const c_void;
            let err = unsafe {
                setsockopt(
                    sock_fd,
                    SOL_CAN_ISOTP,
                    CAN_ISOTP_RECV_FC,
                    rx_flow_control_options_ptr,
                    FLOW_CONTROL_OPTIONS_SIZE.try_into().unwrap(),
                )
            };
            if err == -1 {
                return Err(Error::from(io::Error::last_os_error()));
            }
        }

        // Set LinkLayerOptions
        if let Some(link_layer_options) = link_layer_options {
            let link_layer_options_ptr: *const c_void =
                &link_layer_options as *const _ as *const c_void;
            let err = unsafe {
                setsockopt(
                    sock_fd,
                    SOL_CAN_ISOTP,
                    CAN_ISOTP_LL_OPTS,
                    link_layer_options_ptr,
                    LINK_LAYER_OPTIONS_SIZE.try_into().unwrap(),
                )
            };
            if err == -1 {
                return Err(Error::from(io::Error::last_os_error()));
            }
        }

        // bind it
        let bind_rv;
        unsafe {
            let sockaddr_ptr = &addr as *const CanAddr;
            bind_rv = bind(
                sock_fd,
                sockaddr_ptr as *const sockaddr,
                size_of::<CanAddr>().try_into().unwrap(),
            );
        }

        // FIXME: on fail, close socket (do not leak socketfds)
        if bind_rv == -1 {
            let e = io::Error::last_os_error();
            unsafe {
                close(sock_fd);
            }
            return Err(Error::from(e));
        }

        Ok(Self {
            fd: sock_fd,
            recv_buffer: [0x00; RECV_BUFFER_SIZE],
        })
    }

    fn close(&mut self) -> io::Result<()> {
        unsafe {
            let rv = close(self.fd);
            if rv != -1 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    /// Change socket to non-blocking mode
    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        // retrieve current flags
        let oldfl = unsafe { fcntl(self.fd, F_GETFL) };

        if oldfl == -1 {
            return Err(io::Error::last_os_error());
        }

        let newfl = if nonblocking {
            oldfl | O_NONBLOCK
        } else {
            oldfl & !O_NONBLOCK
        };

        let rv = unsafe { fcntl(self.fd, F_SETFL, newfl) };

        if rv != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Blocking read data
    pub fn read(&mut self) -> io::Result<&[u8]> {
        let buffer_ptr = &mut self.recv_buffer as *mut _ as *mut c_void;

        let read_rv = unsafe { read(self.fd, buffer_ptr, RECV_BUFFER_SIZE) };

        if read_rv < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(&self.recv_buffer[0..read_rv.try_into().unwrap()])
    }

    /// Blocking write a slice of data
    pub fn write(&self, buffer: &[u8]) -> io::Result<()> {
        let write_rv = unsafe {
            let buffer_ptr = buffer as *const _ as *const c_void;
            write(self.fd, buffer_ptr, buffer.len())
        };

        if write_rv != buffer.len().try_into().unwrap() {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }
}

impl AsRawFd for IsoTpSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl FromRawFd for IsoTpSocket {
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
        Self {
            fd,
            recv_buffer: [0x00; RECV_BUFFER_SIZE],
        }
    }
}

impl IntoRawFd for IsoTpSocket {
    fn into_raw_fd(self) -> RawFd {
        self.fd
    }
}

impl Drop for IsoTpSocket {
    fn drop(&mut self) {
        self.close().ok(); // ignore result
    }
}
