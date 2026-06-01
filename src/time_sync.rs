use crate::Time;
use core::net::SocketAddr;
use embassy_executor::Spawner;
use embassy_net::{
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
    udp::{PacketMetadata, UdpSocket},
    Config as NetConfig, IpListenEndpoint, Runner, Stack, StackResources,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Timer};
use esp_hal::{peripherals::WIFI, rng::Rng};
use esp_println::println;
use esp_radio::wifi::{
    sta::StationConfig, Config as WifiConfiguration, ControllerConfig, Interface, Ssid,
    WifiController,
};
use reqwless::{client::HttpClient, request::Method};
use serde::Deserialize;
use sntpc::{get_time, NtpContext};
use sntpc_net_embassy::UdpSocketWrapper;
use sntpc_time_embassy::EmbassyTimestampGenerator;
use static_cell::StaticCell;

pub static CURRENT_TIME: Channel<CriticalSectionRawMutex, crate::Time, 1> = Channel::new();

#[derive(Deserialize)]
struct TimezoneResponse {
    // ip-api.com returns 'offset' in seconds from UTC
    offset: i32,
}

pub async fn setup_time_sync(wifi_peripheral: WIFI<'static>, spawner: Spawner) {
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
    spawner.spawn(connection_task(controller).unwrap());
    spawner.spawn(net_runner_task(runner).unwrap());
    spawner.spawn(sntp_sync_task(stack).unwrap());
}

#[embassy_executor::task]
async fn connection_task(mut controller: WifiController<'static>) {
    println!("Starting Wi-Fi connection task...");

    loop {
        match controller.connect_async().await {
            Ok(_) => {
                println!("Wi-Fi Connected successfully!");
                let _ = controller.wait_for_disconnect_async().await;
                println!("Wi-Fi disconnected! Attempting to reconnect...");
            }
            Err(e) => {
                println!("Connection failed. {} Retrying in 5 seconds...", e);
                Timer::after(Duration::from_millis(5000)).await;
            }
        }
    }
}

#[embassy_executor::task]
async fn net_runner_task(mut runner: Runner<'static, Interface<'static>>) {
    runner.run().await
}

#[embassy_executor::task]
async fn sntp_sync_task(stack: Stack<'static>) {
    println!("Waiting for IP address...");
    loop {
        if stack.is_config_up() {
            if let Some(config) = stack.config_v4() {
                println!("Assigned IP: {}", config.address);
                break;
            }
        }
        Timer::after(Duration::from_millis(500)).await;
    }

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
    let endpoints = dns
        .query("pool.ntp.org", embassy_net::dns::DnsQueryType::A)
        .await
        .unwrap();
    let server_endpoint = SocketAddr::new(endpoints[0].into(), 123);

    let tz_offset = fetch_timezone_offset(stack).await;

    loop {
        println!("Sending SNTP request to {:?}", server_endpoint);

        match get_time(server_endpoint, &socket_wrapper, ntp_context).await {
            Ok(ntp_result) => {
                let ntp_seconds = ntp_result.sec() as i64;

                CURRENT_TIME
                    .send(get_current_time_epoch(ntp_seconds, tz_offset))
                    .await;

                println!("Time Sync OK! Epoch: {}", ntp_seconds);
                Timer::after(Duration::from_secs(3600)).await;
            }
            Err(_e) => {
                println!("SNTP Sync Failed, retrying in 10s...");
                Timer::after(Duration::from_secs(10)).await;
            }
        }
    }
}

async fn fetch_timezone_offset(stack: Stack<'_>) -> i32 {
    static TCP_CLIENT_STATE: StaticCell<TcpClientState<1, 2048, 2048>> = StaticCell::new();
    let tcp_state = TCP_CLIENT_STATE.init(TcpClientState::new());

    let tcp_client = TcpClient::new(stack, &tcp_state);
    let dns_socket = DnsSocket::new(stack);
    let mut rx_buffer = [0u8; 2048];
    let mut client = HttpClient::new(&tcp_client, &dns_socket);

    let mut request = client
        .request(Method::GET, "http://ip-api.com/json/?fields=offset")
        .await
        .unwrap();

    let response = request.send(&mut rx_buffer).await.unwrap();
    let body = response.body().read_to_end().await.unwrap();

    if let Ok((data, _)) = serde_json_core::from_slice::<TimezoneResponse>(body) {
        println!("Got timezone offset: {}", data.offset);
        return data.offset;
    }

    println!("Failed to fetch timezone offset.");
    0
}

fn get_current_time_epoch(utc_epoch: i64, tz_offset_seconds: i32) -> crate::Time {
    let day_seconds = ((utc_epoch + tz_offset_seconds as i64) % 86400 + 86400) % 86400;
    Time::new(
        (day_seconds / 3600) as u8,
        ((day_seconds % 3600) / 60) as u8,
        (day_seconds % 60) as u8,
    )
}
