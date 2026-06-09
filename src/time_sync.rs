use crate::{ui::CURRENT_INFO, Time};
use core::net::SocketAddr;
use embassy_executor::Spawner;
use embassy_net::{
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
    udp::{PacketMetadata, UdpSocket},
    Config as NetConfig, IpListenEndpoint, Runner, Stack, StackResources,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Timer};
use esp_hal::{peripherals::WIFI, rng::Rng};
use esp_println::println;
use esp_radio::wifi::{
    sta::StationConfig, Config as WifiConfiguration, ControllerConfig, Interface, Ssid,
    WifiController,
};
use reqwless::{client::HttpClient, request::Method};
use sntpc::{get_time, NtpContext};
use sntpc_net_embassy::UdpSocketWrapper;
use sntpc_time_embassy::EmbassyTimestampGenerator;
use static_cell::StaticCell;

pub static CURRENT_TIME: Signal<CriticalSectionRawMutex, crate::Time> = Signal::new();
const TIME_REFRESH_INTERVAL: Duration = Duration::from_secs(3600 * 3);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Sleeping,
}

pub fn setup_time_sync(wifi_peripheral: WIFI<'static>, spawner: Spawner) {
    println!("Configuring esp-radio and network stack...");

    let station_config = StationConfig::default()
        .with_ssid(Ssid::from(env!("WIFI_SSID")))
        .with_password(env!("WIFI_PASS").into());

    let controller_config =
        ControllerConfig::default().with_initial_config(WifiConfiguration::Station(station_config));

    let (controller, interfaces) =
        esp_radio::wifi::new(wifi_peripheral, controller_config).unwrap();

    static RESOURCES: static_cell::StaticCell<StackResources<4>> = static_cell::StaticCell::new();

    let rng = Rng::new();
    let seed = (rng.random() as u64) | ((rng.random() as u64) << 32);

    let (stack, runner) = embassy_net::new(
        interfaces.station,
        NetConfig::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::<4>::new()),
        seed,
    );

    println!("Spawning background network tasks...");
    spawner.spawn(net_runner_task(runner).unwrap());
    spawner.spawn(sync_manager_task(controller, stack).unwrap());
}

#[embassy_executor::task]
async fn net_runner_task(mut runner: Runner<'static, Interface<'static>>) {
    runner.run().await
}

#[embassy_executor::task]
async fn sync_manager_task(mut controller: WifiController<'static>, stack: Stack<'static>) {
    let mut tz_offset = 0;
    let mut tz_fetched = false;

    loop {
        println!("Initiating time sync sequence...");

        let mut connected = false;
        const MAX_RETRIES: usize = 5;
        let mut retries = 0;

        while retries < MAX_RETRIES {
            {
                CURRENT_INFO
                    .lock()
                    .await
                    .set_state(ConnectionState::Connecting);
            }

            match controller.connect_async().await {
                Ok(_) => {
                    println!("Wi-Fi Connected!");
                    {
                        CURRENT_INFO
                            .lock()
                            .await
                            .set_state(ConnectionState::Connected);
                    }
                    connected = true;
                    break;
                }
                Err(_e) => {
                    retries += 1;
                    println!("Connection failed ({}/{}): {:?}", retries, MAX_RETRIES, _e);
                    if retries < MAX_RETRIES {
                        Timer::after(Duration::from_secs(10)).await;
                    }
                }
            }
        }

        if !connected {
            println!("Max retries reached. Skipping this sync cycle.");
            {
                let mut info = CURRENT_INFO.lock().await;
                info.set_state(ConnectionState::Disconnected);
                info.clear_ip_address();
            }
        } else {
            // Wait for IP address
            loop {
                if stack.is_config_up() {
                    if let Some(config) = stack.config_v4() {
                        println!("Assigned IP: {}", config.address);
                        CURRENT_INFO
                            .lock()
                            .await
                            .set_ip_address(config.address.address());
                        break;
                    }
                }
                Timer::after(Duration::from_millis(500)).await;
            }

            // One-time timezone fetch
            if !tz_fetched {
                if let Some((offset, tz_name)) = fetch_timezone_offset(stack).await {
                    println!("Fetched timezone: {} ({})", tz_name, offset);
                    CURRENT_INFO.lock().await.set_timezone(tz_name);
                    tz_offset = offset;
                    tz_fetched = true;
                } else {
                    println!("Failed to fetch timezone, using 0");
                }
            }

            // Perform SNTP Sync
            let mut rx_meta = [PacketMetadata::EMPTY; 16];
            let mut rx_buffer = [0u8; 1024];
            let mut tx_meta = [PacketMetadata::EMPTY; 16];
            let mut tx_buffer = [0u8; 1024];
            let mut socket = UdpSocket::new(
                stack,
                &mut rx_meta,
                &mut rx_buffer,
                &mut tx_meta,
                &mut tx_buffer,
            );
            socket
                .bind(IpListenEndpoint {
                    addr: None,
                    port: 12345,
                })
                .unwrap();
            let socket_wrapper = UdpSocketWrapper::from(socket);
            let ntp_context = NtpContext::new(EmbassyTimestampGenerator::default());
            let dns = DnsSocket::new(stack);

            if let Ok(endpoints) = dns
                .query("pool.ntp.org", embassy_net::dns::DnsQueryType::A)
                .await
            {
                let server_endpoint = SocketAddr::new(endpoints[0].into(), 123);
                match get_time(server_endpoint, &socket_wrapper, ntp_context).await {
                    Ok(ntp_result) => {
                        let new_time = get_current_time_epoch(ntp_result.sec() as i64, tz_offset);
                        CURRENT_TIME.signal(new_time);
                        CURRENT_INFO.lock().await.set_sync_time(new_time);
                        println!("Time synced successfully: {}", new_time);
                    }
                    Err(_e) => println!("SNTP Request failed: {:?}", _e),
                }
            }

            println!("Sync complete. Disconnecting to save power...");
            let _ = controller.disconnect_async().await;
            {
                let mut info = CURRENT_INFO.lock().await;
                info.set_state(ConnectionState::Disconnected);
                info.clear_ip_address();
            }
        }

        println!(
            "Entering low-power wait for {} seconds...",
            TIME_REFRESH_INTERVAL.as_secs()
        );
        {
            CURRENT_INFO
                .lock()
                .await
                .set_state(ConnectionState::Sleeping);
        }
        Timer::after(TIME_REFRESH_INTERVAL).await;
    }
}

async fn fetch_timezone_offset(stack: Stack<'_>) -> Option<(i32, heapless::String<32>)> {
    static TCP_CLIENT_STATE: StaticCell<TcpClientState<1, 2048, 2048>> = StaticCell::new();
    let tcp_state = TCP_CLIENT_STATE.init(TcpClientState::new());

    let tcp_client = TcpClient::new(stack, tcp_state);
    let dns_socket = DnsSocket::new(stack);
    let mut rx_buffer = [0u8; 2048];
    let mut client = HttpClient::new(&tcp_client, &dns_socket);

    let mut request = client
        .request(
            Method::GET,
            "http://ip-api.com/line/?fields=offset,timezone",
        )
        .await
        .ok()?;

    let response = request.send(&mut rx_buffer).await.ok()?;
    let body = response.body().read_to_end().await.ok()?;
    let body_str = str::from_utf8(body).ok()?;
    let mut parts = body_str.lines();
    let mut tz_name = heapless::String::<32>::new();
    if tz_name.push_str(parts.next()?).is_err() {
        return None;
    }
    let offset = parts.next()?.trim().parse::<i32>().ok()?;
    Some((offset, tz_name))
}

fn get_current_time_epoch(utc_epoch: i64, tz_offset_seconds: i32) -> crate::Time {
    let day_seconds = ((utc_epoch + tz_offset_seconds as i64) % 86400 + 86400) % 86400;
    let time = Time::new(
        (day_seconds / 3600) as u8,
        ((day_seconds % 3600) / 60) as u8,
        (day_seconds % 60) as u8,
    );
    println!("Parsed time from SNTP: {}", time);
    time
}
