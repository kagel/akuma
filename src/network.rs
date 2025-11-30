// Network stack with device registry
// Uses fixed-size arrays - no heap allocation during init

use smoltcp::iface::{Config, Interface};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress};
use virtio_drivers::device::net::VirtIONetRaw;
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};

use crate::virtio_hal::VirtioHal;

// ============================================================================
// Constants
// ============================================================================

const MAX_INTERFACES: usize = 4;
const MAX_NAME_LEN: usize = 8;

// ============================================================================
// Network Interface Types
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InterfaceType {
    None = 0,
    Loopback = 1,
    Ethernet = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InterfaceStatus {
    Down = 0,
    Up = 1,
}

// ============================================================================
// Network Interface
// ============================================================================

#[derive(Clone, Copy)]
pub struct NetworkInterface {
    pub name: [u8; MAX_NAME_LEN],
    pub iface_type: InterfaceType,
    pub status: InterfaceStatus,
    pub mac: [u8; 6],
}

impl NetworkInterface {
    pub const fn empty() -> Self {
        Self {
            name: [0; MAX_NAME_LEN],
            iface_type: InterfaceType::None,
            status: InterfaceStatus::Down,
            mac: [0; 6],
        }
    }

    pub fn name_str(&self) -> &str {
        let len = self
            .name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(MAX_NAME_LEN);
        core::str::from_utf8(&self.name[..len]).unwrap_or("???")
    }
}

// ============================================================================
// Network State (fixed-size, no allocation)
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NetworkStatus {
    Uninitialized = 0,
    Initializing = 1,
    Ready = 2,
    Error = 3,
}

pub struct NetworkState {
    pub status: NetworkStatus,
    pub interfaces: [NetworkInterface; MAX_INTERFACES],
    pub interface_count: usize,
    pub has_ethernet: bool,
}

impl NetworkState {
    pub const fn new() -> Self {
        Self {
            status: NetworkStatus::Uninitialized,
            interfaces: [NetworkInterface::empty(); MAX_INTERFACES],
            interface_count: 0,
            has_ethernet: false,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.status == NetworkStatus::Ready
    }

    pub fn add_interface(&mut self, name: &[u8], iface_type: InterfaceType, mac: [u8; 6]) -> bool {
        if self.interface_count >= MAX_INTERFACES {
            return false;
        }

        let idx = self.interface_count;
        self.interface_count += 1;

        // Copy name
        for (i, &c) in name.iter().enumerate() {
            if i < MAX_NAME_LEN {
                self.interfaces[idx].name[i] = c;
            }
        }

        self.interfaces[idx].iface_type = iface_type;
        self.interfaces[idx].status = InterfaceStatus::Up;
        self.interfaces[idx].mac = mac;

        if iface_type == InterfaceType::Ethernet {
            self.has_ethernet = true;
        }

        true
    }

    pub fn get_interface(&self, name: &str) -> Option<&NetworkInterface> {
        for i in 0..self.interface_count {
            if self.interfaces[i].name_str() == name {
                return Some(&self.interfaces[i]);
            }
        }
        None
    }
}

// ============================================================================
// Global Network State
// ============================================================================

static mut NETWORK_STATE: NetworkState = NetworkState::new();

/// Check if network is initialized and ready
pub fn is_ready() -> bool {
    unsafe {
        let ptr = core::ptr::addr_of!(NETWORK_STATE);
        (*ptr).is_ready()
    }
}

/// Get network status
pub fn status() -> NetworkStatus {
    unsafe {
        let ptr = core::ptr::addr_of!(NETWORK_STATE);
        (*ptr).status
    }
}

/// Get interface count
pub fn interface_count() -> usize {
    unsafe {
        let ptr = core::ptr::addr_of!(NETWORK_STATE);
        (*ptr).interface_count
    }
}

/// Check if ethernet is available
pub fn has_ethernet() -> bool {
    unsafe {
        let ptr = core::ptr::addr_of!(NETWORK_STATE);
        (*ptr).has_ethernet
    }
}

// ============================================================================
// Virtio-net Device
// ============================================================================

static mut VIRTIO_RX_BUFFER: [u8; 4096] = [0; 4096];
static mut VIRTIO_TX_BUFFER: [u8; 4096] = [0; 4096];

pub struct VirtioNetDevice {
    inner: VirtIONetRaw<VirtioHal, MmioTransport, 16>,
}

pub struct VirtioRxToken {
    len: usize,
}

pub struct VirtioTxToken<'a> {
    device: &'a mut VirtioNetDevice,
}

impl RxToken for VirtioRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        unsafe { f(&mut VIRTIO_RX_BUFFER[..self.len]) }
    }
}

impl<'a> TxToken for VirtioTxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        unsafe {
            let result = f(&mut VIRTIO_TX_BUFFER[..len]);
            let _ = self.device.inner.send(&VIRTIO_TX_BUFFER[..len]);
            result
        }
    }
}

impl Device for VirtioNetDevice {
    type RxToken<'a> = VirtioRxToken;
    type TxToken<'a> = VirtioTxToken<'a>;

    fn receive(&mut self, _ts: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        None
    }

    fn transmit(&mut self, _ts: Instant) -> Option<Self::TxToken<'_>> {
        None
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.medium = Medium::Ethernet;
        caps
    }
}

// ============================================================================
// Logging Helpers
// ============================================================================

#[inline(always)]
fn log(msg: &[u8]) {
    unsafe {
        const UART: *mut u8 = 0x0900_0000 as *mut u8;
        for &c in msg {
            core::ptr::write_volatile(UART, c);
        }
    }
}

fn log_hex_byte(b: u8) {
    let hex = |n: u8| if n < 10 { b'0' + n } else { b'a' + n - 10 };
    log(&[hex((b >> 4) & 0xF), hex(b & 0xF)]);
}

// ============================================================================
// Initialization
// ============================================================================

/// QEMU virt machine virtio MMIO addresses
const VIRTIO_MMIO_ADDRS: [usize; 8] = [
    0x0a000000, 0x0a000200, 0x0a000400, 0x0a000600, 0x0a000800, 0x0a000a00, 0x0a000c00, 0x0a000e00,
];

pub fn init(_dtb_ptr: usize) -> Result<(), &'static str> {
    unsafe {
        let ptr = core::ptr::addr_of_mut!(NETWORK_STATE);
        if (*ptr).status != NetworkStatus::Uninitialized {
            return Err("Network already initialized");
        }
        (*ptr).status = NetworkStatus::Initializing;
    }

    log(b"[Net] init\n");

    // Register lo0 (loopback)
    unsafe {
        let ptr = core::ptr::addr_of_mut!(NETWORK_STATE);
        (*ptr).add_interface(b"lo0", InterfaceType::Loopback, [0; 6]);
    }
    log(b"[Net] lo0: UP\n");

    // Scan for ethernet devices
    for (i, &addr) in VIRTIO_MMIO_ADDRS.iter().enumerate() {
        log(b"[");
        log(&[b'0' + i as u8]);
        log(b"]");

        unsafe {
            // Check device ID (1 = network device)
            let device_id = core::ptr::read_volatile((addr + 0x008) as *const u32);
            if device_id != 1 {
                log(b"- ");
                continue;
            }

            log(b"!\n");

            // Create MMIO transport
            let header_ptr = match core::ptr::NonNull::new(addr as *mut VirtIOHeader) {
                Some(p) => p,
                None => continue,
            };

            let transport = match MmioTransport::new(header_ptr) {
                Ok(t) => t,
                Err(_) => {
                    log(b"[Net] transport fail\n");
                    continue;
                }
            };

            log(b"[Net] virtio init...\n");

            // Initialize VirtIO network device
            let net = match VirtIONetRaw::<VirtioHal, MmioTransport, 16>::new(transport) {
                Ok(n) => n,
                Err(_) => {
                    log(b"[Net] device fail\n");
                    continue;
                }
            };

            let mac = net.mac_address();
            let mut dev = VirtioNetDevice { inner: net };

            log(b"[Net] smoltcp...\n");

            // Create smoltcp interface
            let hw = EthernetAddress::from_bytes(&mac);
            let cfg = Config::new(HardwareAddress::Ethernet(hw));
            let _iface = Interface::new(cfg, &mut dev, Instant::ZERO);

            // Log MAC address
            log(b"[Net] eth0: ");
            for (j, &b) in mac.iter().enumerate() {
                if j > 0 {
                    log(b":");
                }
                log_hex_byte(b);
            }
            log(b" UP\n");

            // Register interface
            let ptr = core::ptr::addr_of_mut!(NETWORK_STATE);
            (*ptr).add_interface(b"eth0", InterfaceType::Ethernet, mac.into());
            break; // Only handle first network device for now
        }
    }

    // Mark as ready
    unsafe {
        let ptr = core::ptr::addr_of_mut!(NETWORK_STATE);
        (*ptr).status = NetworkStatus::Ready;
    }

    log(b"[Net] Ready\n");
    Ok(())
}

// ============================================================================
// Interface Listing
// ============================================================================

pub fn list_interfaces() {
    unsafe {
        let ptr = core::ptr::addr_of!(NETWORK_STATE);
        let state = &*ptr;

        log(b"\nInterfaces:\n");
        for i in 0..state.interface_count {
            let iface = &state.interfaces[i];

            // Print name
            for &c in iface.name.iter() {
                if c != 0 {
                    log(&[c]);
                }
            }
            log(b": ");

            // Print type and details
            match iface.iface_type {
                InterfaceType::Loopback => log(b"loopback"),
                InterfaceType::Ethernet => {
                    log(b"ethernet ");
                    for (j, &b) in iface.mac.iter().enumerate() {
                        if j > 0 {
                            log(b":");
                        }
                        log_hex_byte(b);
                    }
                }
                InterfaceType::None => log(b"none"),
            }

            // Print status
            match iface.status {
                InterfaceStatus::Up => log(b" UP"),
                InterfaceStatus::Down => log(b" DOWN"),
            }
            log(b"\n");
        }
        log(b"\n");
    }
}

// ============================================================================
// Polling
// ============================================================================

static mut TOTAL_PACKETS_RX: u64 = 0;
static mut TOTAL_PACKETS_TX: u64 = 0;
static mut POLL_COUNT: u64 = 0;

/// Poll network interfaces for packets
/// Returns number of packets processed in this poll cycle
pub fn poll() -> usize {
    let mut packets_processed = 0;

    unsafe {
        let poll_count = core::ptr::addr_of_mut!(POLL_COUNT);
        *poll_count += 1;

        // TODO: Actually poll virtio RX queue here
        // For now, just log periodically to show we're running
        if *poll_count % 1000000 == 0 {
            log(b"[Net] poll #");
            log_u64(*poll_count);
            log(b"\n");
        }
    }

    // TODO: Process incoming packets from virtio
    // packets_processed += process_rx_queue();

    // TODO: Process outgoing packets
    // packets_processed += process_tx_queue();

    packets_processed
}

/// Get total received packet count
pub fn total_rx_packets() -> u64 {
    unsafe {
        let ptr = core::ptr::addr_of!(TOTAL_PACKETS_RX);
        *ptr
    }
}

/// Get total transmitted packet count  
pub fn total_tx_packets() -> u64 {
    unsafe {
        let ptr = core::ptr::addr_of!(TOTAL_PACKETS_TX);
        *ptr
    }
}

/// Get total poll count
pub fn poll_count() -> u64 {
    unsafe {
        let ptr = core::ptr::addr_of!(POLL_COUNT);
        *ptr
    }
}

fn log_u64(mut n: u64) {
    if n == 0 {
        log(b"0");
        return;
    }

    let mut buf = [0u8; 20];
    let mut i = 0;

    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }

    // Reverse and print
    while i > 0 {
        i -= 1;
        log(&[buf[i]]);
    }
}
