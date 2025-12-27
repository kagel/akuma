//! SSH Server - Concurrent Multi-Session Accept Loop
//!
//! Manages the SSH server accept loop that handles multiple
//! concurrent SSH sessions. Each connection runs in parallel.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use embassy_net::tcp::TcpSocket;
use embassy_net::Stack;
use embassy_time::Duration;

use crate::async_net::TcpStream;
use crate::console;
use crate::ssh;

// ============================================================================
// Constants
// ============================================================================

const SSH_PORT: u16 = 22;
const MAX_CONNECTIONS: usize = 8;
const TCP_RX_BUFFER_SIZE: usize = 4096;
const TCP_TX_BUFFER_SIZE: usize = 4096;

// ============================================================================
// Connection State
// ============================================================================

/// Active SSH connection being handled
struct ActiveConnection {
    future: Pin<Box<dyn Future<Output = ()>>>,
    id: usize,
}

// ============================================================================
// SSH Server with Concurrent Connections
// ============================================================================

/// Run the SSH server with support for multiple concurrent connections
pub async fn run(stack: Stack<'static>) {
    log("[SSH Server] Starting SSH server on port 22...\n");
    log(&alloc::format!(
        "[SSH Server] Max concurrent connections: {}\n",
        MAX_CONNECTIONS
    ));
    log("[SSH Server] Connect with: ssh -o StrictHostKeyChecking=no user@localhost -p 2222\n");

    // Initialize shared host key
    ssh::init_host_key();

    // Active connections
    let mut connections: Vec<ActiveConnection> = Vec::new();
    let mut next_id: usize = 0;

    // Pre-allocate a listening socket (reused when no connections)
    let mut listen_socket: Option<TcpSocket<'static>> = None;

    // Create waker for manual polling
    static VTABLE: RawWakerVTable = RawWakerVTable::new(
        |_| RawWaker::new(core::ptr::null(), &VTABLE),
        |_| {},
        |_| {},
        |_| {},
    );

    loop {
        let raw_waker = RawWaker::new(core::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw_waker) };
        let mut cx = Context::from_waker(&waker);

        // =====================================================================
        // Poll all active connections first
        // =====================================================================
        let mut i = 0;
        while i < connections.len() {
            match connections[i].future.as_mut().poll(&mut cx) {
                Poll::Ready(()) => {
                    let conn = connections.swap_remove(i);
                    log(&alloc::format!(
                        "[SSH Server] Connection {} ended (active: {})\n",
                        conn.id,
                        connections.len()
                    ));
                }
                Poll::Pending => {
                    i += 1;
                }
            }
        }

        // =====================================================================
        // Accept new connection if we have capacity
        // =====================================================================
        if connections.len() < MAX_CONNECTIONS {
            // Ensure we have a listening socket
            if listen_socket.is_none() {
                listen_socket = Some(create_listen_socket(stack));
            }

            if let Some(ref mut socket) = listen_socket {
                // If no active connections, we can block on accept
                if connections.is_empty() {
                    match socket.accept(SSH_PORT).await {
                        Ok(()) => {
                            let id = next_id;
                            next_id = next_id.wrapping_add(1);

                            log(&alloc::format!(
                                "[SSH Server] Accepted connection {} (active: 1)\n",
                                id
                            ));

                            // Take the socket and create a new one for listening
                            let connected_socket = listen_socket.take().unwrap();
                            let stream = TcpStream::from_socket(connected_socket);
                            let future = Box::pin(handle_connection_wrapper(stream, id));
                            connections.push(ActiveConnection { future, id });
                        }
                        Err(e) => {
                            log(&alloc::format!("[SSH Server] Accept error: {:?}\n", e));
                            // Reset the socket
                            listen_socket = None;
                        }
                    }
                } else {
                    // Have active connections - use timeout to avoid blocking
                    match embassy_time::with_timeout(
                        Duration::from_millis(10),
                        socket.accept(SSH_PORT),
                    )
                    .await
                    {
                        Ok(Ok(())) => {
                            let id = next_id;
                            next_id = next_id.wrapping_add(1);

                            log(&alloc::format!(
                                "[SSH Server] Accepted connection {} (active: {})\n",
                                id,
                                connections.len() + 1
                            ));

                            // Take the socket and create a new one for listening
                            let connected_socket = listen_socket.take().unwrap();
                            let stream = TcpStream::from_socket(connected_socket);
                            let future = Box::pin(handle_connection_wrapper(stream, id));
                            connections.push(ActiveConnection { future, id });
                        }
                        Ok(Err(e)) => {
                            log(&alloc::format!("[SSH Server] Accept error: {:?}\n", e));
                            listen_socket = None;
                        }
                        Err(_) => {
                            // Timeout - no new connection, that's okay
                            // Socket is still in listen state, but we need to abort and recreate
                            // because embassy-net accept future consumed the socket state
                            socket.abort();
                            listen_socket = None;
                        }
                    }
                }
            }
        } else {
            // At max capacity, just yield briefly
            embassy_time::Timer::after(Duration::from_millis(1)).await;
        }

        // Check embassy time alarms
        crate::embassy_time_driver::on_timer_interrupt();
    }
}

/// Create a new socket for listening
fn create_listen_socket(stack: Stack<'static>) -> TcpSocket<'static> {
    let rx_buffer = alloc::vec![0u8; TCP_RX_BUFFER_SIZE].into_boxed_slice();
    let tx_buffer = alloc::vec![0u8; TCP_TX_BUFFER_SIZE].into_boxed_slice();
    let rx_ref: &'static mut [u8] = Box::leak(rx_buffer);
    let tx_ref: &'static mut [u8] = Box::leak(tx_buffer);

    let mut socket = TcpSocket::new(stack, rx_ref, tx_ref);
    socket.set_timeout(Some(Duration::from_secs(60)));
    socket
}

/// Wrapper for handle_connection that logs start/end
async fn handle_connection_wrapper(stream: TcpStream, id: usize) {
    log(&alloc::format!("[SSH {}] Starting session\n", id));
    ssh::handle_connection(stream).await;
    log(&alloc::format!("[SSH {}] Session ended\n", id));
}

// ============================================================================
// Logging
// ============================================================================

fn log(msg: &str) {
    console::print(msg);
}
