//! UDS (unified diagnostic protocol) example for reading a data by identifer.
//! Run the following server for testing.
//! https://github.com/zombieCraig/uds-server

use socketcan_isotp::{self, IsoTpSocket, StandardId};
use std::sync::mpsc;

fn main() -> Result<(), socketcan_isotp::Error> {
    let (tx, rx) = mpsc::channel();

    // Reader
    let mut reader_tp_socket = IsoTpSocket::open(
        "vcan0",
        StandardId::new(0x7E8).expect("Invalid dst CAN ID"),
        StandardId::new(0x77A).expect("Invalid src CAN ID"),
    )?;
    std::thread::spawn(move || loop {
        let buffer = reader_tp_socket.read().expect("Failed to read from socket");
        tx.send(buffer.to_vec()).expect("Receiver deallocated");
    });

    let tp_socket = IsoTpSocket::open(
        "vcan0",
        StandardId::new(0x77A).expect("Invalid src CAN ID"),
        StandardId::new(0x7E0).expect("Invalid dst CAN ID"),
    )?;

    // 0x22 - Service Identifier for "Read data by identifier" request
    // 0xF189 - Data identifer - VehicleManufacturerECUSoftwareVersionNumberDataIdentifier
    tp_socket.write(&[0x22, 0xF1, 0x89])?;

    println!("Sent read data by identifier 0xF189 - VehicleManufacturerECUSoftwareVersionNumberDataIdentifier");

    loop {
        let recv_buffer = rx.recv().expect("Failed to receive");
        // 0x62 - Service Identifier for "Read data by identifier" response
        // 0xF189 - Data identifer - VehicleManufacturerECUSoftwareVersionNumberDataIdentifier
        if recv_buffer[0..=2] != [0x62, 0xF1, 0x89] {
            println!("Skipping: {:X?}", recv_buffer);
        } else {
            println!("Response: {:X?}", &recv_buffer[3..]);
        }
    }
}
