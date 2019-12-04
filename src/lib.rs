//! SocketCAN support.
//!
//! The Linux kernel supports using CAN-devices through a network-like API
//! (see https://www.kernel.org/doc/Documentation/networking/can.txt). This
//! crate allows easy access to this functionality without having to wrestle
//! libc calls.
//!
//! # An introduction to CAN
//!
//! The CAN bus was originally designed to allow microcontrollers inside a
//! vehicle to communicate over a single shared bus. Messages called
//! *frames* are multicast to all devices on the bus.
//!
//! Every frame consists of an ID and a payload of up to 8 bytes. If two
//! devices attempt to send a frame at the same time, the device with the
//! higher ID will notice the conflict, stop sending and reattempt to sent its
//! frame in the next time slot. This means that the lower the ID, the higher
//! the priority. Since most devices have a limited buffer for outgoing frames,
//! a single device with a high priority (== low ID) can block communication
//! on that bus by sending messages too fast.
//!
//! The Linux socketcan subsystem makes the CAN bus available as a regular
//! networking device. Opening an network interface allows receiving all CAN
//! messages received on it. A device CAN be opened multiple times, every
//! client will receive all CAN frames simultaneously.
//!
//! Similarly, CAN frames can be sent to the bus by multiple client
//! simultaneously as well.
//!
//! # Hardware and more information
//!
//! More information on CAN [can be found on Wikipedia](). When not running on
//! an embedded platform with already integrated CAN components,
//! [Thomas Fischl's USBtin](http://www.fischl.de/usbtin/) (see
//! [section 2.4](http://www.fischl.de/usbtin/#socketcan)) is one of many ways
//! to get started.
//!
//! # RawFd
//!
//! Raw access to the underlying file descriptor and construction through
//! is available through the `AsRawFd`, `IntoRawFd` and `FromRawFd`
//! implementations.
use bitflags::bitflags;
use libc::{
    bind, c_int, c_short, c_uint, c_void, close, fcntl, read, setsockopt, sockaddr, socket, write,
    EINPROGRESS, F_GETFL, F_SETFL, O_NONBLOCK, SOCK_DGRAM,
};
use nix::net::if_::if_nametoindex;
use std::convert::TryFrom;
use std::mem::size_of;
use std::num::TryFromIntError;
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
use std::time::Duration;
use std::{error, fmt, io};

/// Check an error return value for timeouts.
///
/// Due to the fact that timeouts are reported as errors, calling `read_frame`
/// on a socket with a timeout that does not receive a frame in time will
/// result in an error being returned. This trait adds a `should_retry` method
/// to `Error` and `Result` to check for this condition.
pub trait ShouldRetry {
    /// Check for timeout
    ///
    /// If `true`, the error is probably due to a timeout.
    fn should_retry(&self) -> bool;
}

impl ShouldRetry for io::Error {
    fn should_retry(&self) -> bool {
        match self.kind() {
            // EAGAIN, EINPROGRESS and EWOULDBLOCK are the three possible codes
            // returned when a timeout occurs. the stdlib already maps EAGAIN
            // and EWOULDBLOCK os WouldBlock
            io::ErrorKind::WouldBlock => true,
            // however, EINPROGRESS is also valid
            io::ErrorKind::Other => {
                if let Some(i) = self.raw_os_error() {
                    i == EINPROGRESS
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

impl<E: fmt::Debug> ShouldRetry for io::Result<E> {
    fn should_retry(&self) -> bool {
        if let Err(ref e) = *self {
            e.should_retry()
        } else {
            false
        }
    }
}

// constants stolen from C headers
pub const AF_CAN: c_int = 29;
pub const PF_CAN: c_int = 29;

/// ISO 15765-2 Transport Protocol
pub const CAN_ISOTP: c_int = 6;

pub const SOL_CAN_BASE: c_int = 100;

pub const SOL_CAN_ISOTP: c_int = SOL_CAN_BASE + CAN_ISOTP;

/// pass struct can_isotp_fc_options
pub const CAN_ISOTP_RECV_FC: c_int = 2;

/// pass __u32 value in nano secs
/// use this time instead of value
/// provided in FC from the receiver
pub const CAN_ISOTP_TX_STMIN: c_int = 3;

/// pass __u32 value in nano secs  
/// ignore received CF frames which
/// timestamps differ less than val
pub const CAN_ISOTP_RX_STMIN: c_int = 4;

/// pass struct can_isotp_ll_options
pub const CAN_ISOTP_LL_OPTS: c_int = 5;

bitflags! {
    pub struct IsoTpBehaviour: u32 {
        /* listen only (do not send FC) */
        const CAN_ISOTP_LISTEN_MODE = 0x001;
        /* enable extended addressing */
        const CAN_ISOTP_EXTEND_ADDR	= 0x002;
        /* enable CAN frame padding tx path */
        const CAN_ISOTP_TX_PADDING	= 0x004;
        /* enable CAN frame padding rx path */
        const CAN_ISOTP_RX_PADDING	= 0x008;
        /* check received CAN frame padding */
        const CAN_ISOTP_CHK_PAD_LEN	= 0x010;
        /* check received CAN frame padding */
        const CAN_ISOTP_CHK_PAD_DATA = 0x020;
        /* half duplex error state handling */
        const CAN_ISOTP_HALF_DUPLEX = 0x040;
        /* ignore stmin from received FC */
        const CAN_ISOTP_FORCE_TXSTMIN = 0x080;
        /* ignore CFs depending on rx stmin */
        const CAN_ISOTP_FORCE_RXSTMIN = 0x100;
        /* different rx extended addressing */
        const CAN_ISOTP_RX_EXT_ADDR = 0x200;
    }
}

/// if set, indicate 29 bit extended format
pub const EFF_FLAG: u32 = 0x80000000;

/// remote transmission request flag
pub const RTR_FLAG: u32 = 0x40000000;

/// error flag
pub const ERR_FLAG: u32 = 0x20000000;

/// valid bits in standard frame id
pub const SFF_MASK: u32 = 0x000007ff;

/// valid bits in extended frame id
pub const EFF_MASK: u32 = 0x1fffffff;

/// valid bits in error frame
pub const ERR_MASK: u32 = 0x1fffffff;

/// an error mask that will cause SocketCAN to report all errors
pub const ERR_MASK_ALL: u32 = ERR_MASK;

/// an error mask that will cause SocketCAN to silently drop all errors
pub const ERR_MASK_NONE: u32 = 0;

#[derive(Debug)]
#[repr(C)]
struct CanAddr {
    _af_can: c_short,
    if_index: c_int, // address familiy,
    rx_id: u32,
    tx_id: u32,
}

#[repr(C)]
pub struct CanIsoTpOptions {
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

impl CanIsoTpOptions {
    fn new(
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
    fn get_flags(&self) -> Option<IsoTpBehaviour> {
        IsoTpBehaviour::from_bits(self.flags)
    }

    /// set flags for isotp behaviour.
    fn set_flags(&mut self, flags: IsoTpBehaviour) {
        self.flags = flags.bits();
    }

    /// get frame transmission time (N_As/N_Ar)
    fn get_frame_txtime(&self) -> Duration {
        Duration::from_nanos(self.frame_txtime.into())
    }

    /// set frame transmission time (N_As/N_Ar)
    fn set_frame_txtime(&mut self, frame_txtime: Duration) -> Result<(), TryFromIntError> {
        self.frame_txtime = u32::try_from(frame_txtime.as_nanos())?;
        Ok(())
    }

    /// get frame transmission time (N_As/N_Ar)
    fn get_ext_address(&self) -> u8 {
        self.ext_address
    }

    /// set address for extended addressing
    fn set_ext_address(&mut self, ext_address: u8) {
        self.ext_address = ext_address;
    }

    /// get address for extended addressing
    fn get_txpad_content(&self) -> u8 {
        self.txpad_content
    }

    /// set content of padding byte (tx)
    fn set_txpad_content(&mut self, txpad_content: u8) {
        self.txpad_content = txpad_content;
    }

    /// get content of padding byte (rx)
    fn get_rxpad_content(&self) -> u8 {
        self.rxpad_content
    }

    /// set content of padding byte (rx)
    fn set_rxpad_content(&mut self, rxpad_content: u8) {
        self.rxpad_content = rxpad_content;
    }

    /// get address for extended addressing
    fn get_rx_ext_address(&self) -> u8 {
        self.rx_ext_address
    }

    /// set address for extended addressing
    fn set_rx_ext_address(&mut self, rx_ext_address: u8) {
        self.rx_ext_address = rx_ext_address;
    }
}

impl Default for CanIsoTpOptions {
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

#[repr(C)]
pub struct CanIsotpFcOptions {
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

#[repr(C)]
pub struct CanIsoTpLlOptions {
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

#[derive(Debug)]
/// Errors opening socket
pub enum IsoTpSocketOpenError {
    /// Device could not be found
    LookupError(nix::Error),

    /// System error while trying to look up device name
    IOError(io::Error),
}

impl fmt::Display for IsoTpSocketOpenError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            IsoTpSocketOpenError::LookupError(ref e) => write!(f, "CAN Device not found: {}", e),
            IsoTpSocketOpenError::IOError(ref e) => write!(f, "IO: {}", e),
        }
    }
}

impl error::Error for IsoTpSocketOpenError {
    fn description(&self) -> &str {
        match *self {
            IsoTpSocketOpenError::LookupError(_) => "can device not found",
            IsoTpSocketOpenError::IOError(ref e) => e.description(),
        }
    }

    fn cause(&self) -> Option<&dyn error::Error> {
        match *self {
            IsoTpSocketOpenError::LookupError(ref e) => Some(e),
            IsoTpSocketOpenError::IOError(ref e) => Some(e),
        }
    }
}

impl From<nix::Error> for IsoTpSocketOpenError {
    fn from(err: nix::Error) -> Self {
        IsoTpSocketOpenError::LookupError(err)
    }
}

#[derive(Debug, Copy, Clone)]
/// Error that occurs when creating CAN packets
pub enum ConstructionError {
    /// CAN ID was outside the range of valid IDs
    IDTooLarge,
    /// More than 8 Bytes of payload data were passed in
    TooMuchData,
}

impl fmt::Display for ConstructionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ConstructionError::IDTooLarge => write!(f, "CAN ID too large"),
            ConstructionError::TooMuchData => {
                write!(f, "Payload is larger than CAN maximum of 8 bytes")
            }
        }
    }
}

impl error::Error for ConstructionError {
    fn description(&self) -> &str {
        match *self {
            ConstructionError::IDTooLarge => "can id too large",
            ConstructionError::TooMuchData => "too much data",
        }
    }
}

// impl From<nix::Error> for IsoTpSocketOpenError {
//     fn from(e: nix::Error) -> IsoTpSocketOpenError {
//         IsoTpSocketOpenError::LookupError(e)
//     }
// }

impl From<io::Error> for IsoTpSocketOpenError {
    fn from(e: io::Error) -> IsoTpSocketOpenError {
        IsoTpSocketOpenError::IOError(e)
    }
}

/// A socket for a CAN device.
///
/// Will be closed upon deallocation. To close manually, use std::drop::Drop.
/// Internally this is just a wrapped file-descriptor.
#[derive(Debug)]
pub struct IsoTpSocket {
    fd: c_int,
}

impl IsoTpSocket {
    /// Open a named CAN device.
    ///
    /// Usually the more common case, opens a socket can device by name, such
    /// as "vcan0" or "socan0".
    pub fn open(ifname: &str) -> Result<IsoTpSocket, IsoTpSocketOpenError> {
        let if_index = if_nametoindex(ifname)?;
        IsoTpSocket::open_if(if_index)
    }

    /// Open CAN device by interface number.
    ///
    /// Opens a CAN device by kernel interface number.
    pub fn open_if(if_index: c_uint) -> Result<IsoTpSocket, IsoTpSocketOpenError> {
        let addr = CanAddr {
            _af_can: AF_CAN as c_short,
            if_index: if_index as c_int,
            rx_id: 0, // ?
            tx_id: 0, // ?
        };

        // open socket
        let sock_fd;
        unsafe {
            sock_fd = socket(PF_CAN, SOCK_DGRAM, CAN_ISOTP);
        }

        if sock_fd == -1 {
            return Err(IsoTpSocketOpenError::from(io::Error::last_os_error()));
        }

        // bind it
        let bind_rv;
        unsafe {
            let sockaddr_ptr = &addr as *const CanAddr;
            bind_rv = bind(
                sock_fd,
                sockaddr_ptr as *const sockaddr,
                size_of::<CanAddr>() as u32,
            );
        }

        // FIXME: on fail, close socket (do not leak socketfds)
        if bind_rv == -1 {
            let e = io::Error::last_os_error();
            unsafe {
                close(sock_fd);
            }
            return Err(IsoTpSocketOpenError::from(e));
        }

        Ok(IsoTpSocket { fd: sock_fd })
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

    /// Blocking read a single can frame.
    pub fn read_frame(&self) -> io::Result<CanFrame> {
        let mut frame = CanFrame {
            _id: 0,
            _data_len: 0,
            _pad: 0,
            _res0: 0,
            _res1: 0,
            _data: [0; 8],
        };

        let read_rv = unsafe {
            let frame_ptr = &mut frame as *mut CanFrame;
            read(self.fd, frame_ptr as *mut c_void, size_of::<CanFrame>())
        };

        if read_rv as usize != size_of::<CanFrame>() {
            return Err(io::Error::last_os_error());
        }

        Ok(frame)
    }

    /// Write a single can frame.
    ///
    /// Note that this function can fail with an `EAGAIN` error or similar.
    /// Use `write_frame_insist` if you need to be sure that the message got
    /// sent or failed.
    pub fn write_frame(&self, frame: &CanFrame) -> io::Result<()> {
        // not a mutable reference needed (see std::net::UdpSocket) for
        // a comparison
        // debug!("Sending: {:?}", frame);

        let write_rv = unsafe {
            let frame_ptr = frame as *const CanFrame;
            write(self.fd, frame_ptr as *const c_void, size_of::<CanFrame>())
        };

        if write_rv as usize != size_of::<CanFrame>() {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }

    /// Blocking write a single can frame, retrying until it gets sent
    /// successfully.
    pub fn write_frame_insist(&self, frame: &CanFrame) -> io::Result<()> {
        loop {
            match self.write_frame(frame) {
                Ok(v) => return Ok(v),
                Err(e) => {
                    if !e.should_retry() {
                        return Err(e);
                    }
                }
            }
        }
    }
}

impl AsRawFd for IsoTpSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl FromRawFd for IsoTpSocket {
    unsafe fn from_raw_fd(fd: RawFd) -> IsoTpSocket {
        IsoTpSocket { fd: fd }
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

/// CanFrame
///
/// Uses the same memory layout as the underlying kernel struct for performance
/// reasons.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct CanFrame {
    /// 32 bit CAN_ID + EFF/RTR/ERR flags
    _id: u32,

    /// data length. Bytes beyond are not valid
    _data_len: u8,

    /// padding
    _pad: u8,

    /// reserved
    _res0: u8,

    /// reserved
    _res1: u8,

    /// buffer for data
    _data: [u8; 8],
}

impl CanFrame {
    pub fn new(id: u32, data: &[u8], rtr: bool, err: bool) -> Result<CanFrame, ConstructionError> {
        let mut _id = id;

        if data.len() > 8 {
            return Err(ConstructionError::TooMuchData);
        }

        if id > EFF_MASK {
            return Err(ConstructionError::IDTooLarge);
        }

        // set EFF_FLAG on large message
        if id > SFF_MASK {
            _id |= EFF_FLAG;
        }

        if rtr {
            _id |= RTR_FLAG;
        }

        if err {
            _id |= ERR_FLAG;
        }

        let mut full_data = [0; 8];

        // not cool =/
        for (n, c) in data.iter().enumerate() {
            full_data[n] = *c;
        }

        Ok(CanFrame {
            _id: _id,
            _data_len: data.len() as u8,
            _pad: 0,
            _res0: 0,
            _res1: 0,
            _data: full_data,
        })
    }

    /// Return the actual CAN ID (without EFF/RTR/ERR flags)
    #[inline]
    pub fn id(&self) -> u32 {
        if self.is_extended() {
            self._id & EFF_MASK
        } else {
            self._id & SFF_MASK
        }
    }

    /// Return the error message
    #[inline]
    pub fn err(&self) -> u32 {
        self._id & ERR_MASK
    }

    /// Check if frame uses 29 bit extended frame format
    #[inline]
    pub fn is_extended(&self) -> bool {
        self._id & EFF_FLAG != 0
    }

    /// Check if frame is an error message
    #[inline]
    pub fn is_error(&self) -> bool {
        self._id & ERR_FLAG != 0
    }

    /// Check if frame is a remote transmission request
    #[inline]
    pub fn is_rtr(&self) -> bool {
        self._id & RTR_FLAG != 0
    }

    /// A slice into the actual data. Slice will always be <= 8 bytes in length
    #[inline]
    pub fn data(&self) -> &[u8] {
        &self._data[..(self._data_len as usize)]
    }
}

// impl fmt::UpperHex for CanFrame {
//     fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
//         write!(f, "{:X}#", self.id())?;

//         let mut parts = self.data().iter().map(|v| format!("{:02X}", v));

//         let sep = if f.alternate() { " " } else { "" };
//         write!(f, "{}", parts.join(sep))
//     }
// }

/// CanFilter
///
/// Contains an internal id and mask. Packets are considered to be matched by
/// a filter if `received_id & mask == filter_id & mask` holds true.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct CanFilter {
    _id: u32,
    _mask: u32,
}

impl CanFilter {
    /// Construct a new CAN filter.
    pub fn new(id: u32, mask: u32) -> Result<CanFilter, ConstructionError> {
        Ok(CanFilter {
            _id: id,
            _mask: mask,
        })
    }
}
