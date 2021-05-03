#![deny(rust_2018_idioms)]

/*!
# Introduction

pluto is called by sundog to generate settings required by Kubernetes.
This is done dynamically because we require access to dynamic networking
setup information.

It makes calls to IMDS to get meta data:

- Cluster DNS
- Node IP
*/

use reqwest::blocking::Client;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::string::String;
use std::{env, process};

use snafu::{OptionExt, ResultExt};

// This is the default DNS unless our CIDR block begins with "10."
const DEFAULT_DNS_CLUSTER_IP: &str = "10.100.0.10";
// If our CIDR block begins with "10." this is our DNS.
const DEFAULT_10_RANGE_DNS_CLUSTER_IP: &str = "172.20.0.10";

// Instance Meta Data Service
const IMDS_BASE_URL: &str = "http://169.254.169.254/2018-09-24";
// Currently only able to get fetch session tokens from `latest`
// FIXME Pin to a date version that supports IMDSv2 once such a date version is available.
const IMDS_SESSION_TOKEN_ENDPOINT: &str = "http://169.254.169.254/latest/api/token";
const IMDS_NODE_IPV4_ENDPOINT: &str = "http://169.254.169.254/2018-09-24/meta-data/local-ipv4";
const IMDS_MAC_ENDPOINT: &str =
    "http://169.254.169.254/2018-09-24/meta-data/network/interfaces/macs";
const IMDS_INSTANCE_TYPE_ENDPOINT: &str =
    "http://169.254.169.254/2018-09-24/meta-data/instance-type";

const ENI_MAX_PODS_PATH: &str = "/usr/share/eks/eni-max-pods";

mod error {
    use snafu::Snafu;

    // Taken from sundog.
    fn code(source: &reqwest::Error) -> String {
        source
            .status()
            .as_ref()
            .map(|i| i.as_str())
            .unwrap_or("Unknown")
            .to_string()
    }

    #[derive(Debug, Snafu)]
    #[snafu(visibility = "pub(super)")]
    pub(super) enum PlutoError {
        #[snafu(display("Error {}ing '{}': {}", method, uri, source))]
        ImdsRequest {
            method: String,
            uri: String,
            source: reqwest::Error,
        },

        #[snafu(display("Error '{}' from '{}': {}", code(&source), uri, source))]
        ImdsResponse { uri: String, source: reqwest::Error },

        #[snafu(display("Error getting text response from {}: {}", uri, source))]
        ImdsText { uri: String, source: reqwest::Error },

        #[snafu(display("Error deserializing response into JSON from {}: {}", uri, source))]
        ImdsJson {
            uri: String,
            source: serde_json::error::Error,
        },

        #[snafu(display(
            "Error serializing to JSON from command output '{}': {}",
            output,
            source
        ))]
        OutputJson {
            output: String,
            source: serde_json::error::Error,
        },

        #[snafu(display(
            "Missing 'region' key in Instance Identity Document from IMDS: {}",
            uri
        ))]
        MissingRegion { uri: String },

        #[snafu(display("Missing MAC address from IMDS: {}", uri))]
        MissingMac { uri: String },

        #[snafu(display("Invalid machine architecture, not one of 'x86_64' or 'aarch64'"))]
        UnknownArchitecture,

        #[snafu(display("Failed to open eni-max-pods file at {}: {}", path, source))]
        EniMaxPodsFile {
            path: &'static str,
            source: std::io::Error,
        },

        #[snafu(display("Failed to parse setting {} as u32: {}", setting, source))]
        ParseToU32 {
            setting: String,
            source: std::num::ParseIntError,
        },

        #[snafu(display("Failed to read line: {}", source))]
        IoReadLine { source: std::io::Error },

        #[snafu(display(
            "Unable to find maximum number of pods supported for instance-type {}",
            instance_type
        ))]
        NoInstanceTypeMaxPods { instance_type: String },
    }
}

use error::PlutoError;

type Result<T> = std::result::Result<T, PlutoError>;

fn get_text_from_imds(client: &Client, uri: &str, session_token: &str) -> Result<String> {
    client
        .get(uri)
        .header("X-aws-ec2-metadata-token", session_token)
        .send()
        .context(error::ImdsRequest { method: "GET", uri })?
        .error_for_status()
        .context(error::ImdsResponse { uri })?
        .text()
        .context(error::ImdsText { uri })
}

fn get_max_pods(client: &Client, session_token: &str) -> Result<String> {
    let instance_type = get_text_from_imds(&client, IMDS_INSTANCE_TYPE_ENDPOINT, session_token)?;
    // Find the corresponding maximum number of pods supported by this instance type
    let file = BufReader::new(
        File::open(ENI_MAX_PODS_PATH).context(error::EniMaxPodsFile {
            path: ENI_MAX_PODS_PATH,
        })?,
    );
    for line in file.lines() {
        let line = line.context(error::IoReadLine)?;
        // Skip the comments in the file
        if line.trim_start().starts_with('#') {
            continue;
        }
        let tokens: Vec<_> = line.split_whitespace().collect();
        if tokens.len() == 2 && tokens[0] == instance_type {
            return Ok(tokens[1].to_string());
        }
    }
    error::NoInstanceTypeMaxPods { instance_type }.fail()
}

fn get_cluster_dns_ip(client: &Client, session_token: &str) -> Result<String> {
    let uri = IMDS_MAC_ENDPOINT;
    let macs = get_text_from_imds(&client, uri, session_token)?;
    // Take the first (primary) MAC address. Others will exist from attached ENIs.
    let mac = macs.split('\n').next().context(error::MissingMac { uri })?;

    // Infer the cluster DNS based on our CIDR blocks.
    let mac_cidr_blocks_uri = format!(
        "{}/meta-data/network/interfaces/macs/{}/vpc-ipv4-cidr-blocks",
        IMDS_BASE_URL, mac
    );
    let mac_cidr_blocks = get_text_from_imds(&client, &mac_cidr_blocks_uri, session_token)?;

    let dns = if mac_cidr_blocks.starts_with("10.") {
        DEFAULT_10_RANGE_DNS_CLUSTER_IP
    } else {
        DEFAULT_DNS_CLUSTER_IP
    }
    .to_string();

    Ok(dns)
}

fn get_node_ip(client: &Client, session_token: &str) -> Result<String> {
    get_text_from_imds(&client, IMDS_NODE_IPV4_ENDPOINT, session_token)
}

/// Print usage message.
fn usage() -> ! {
    let program_name = env::args().next().unwrap_or_else(|| "program".to_string());
    eprintln!(
        r"Usage: {} [max-pods | cluster-dns-ip | node-ip]",
        program_name
    );
    process::exit(1);
}

/// Parses args for the setting key name.
fn parse_args(mut args: env::Args) -> String {
    args.nth(1).unwrap_or_else(|| usage())
}

fn run() -> Result<()> {
    let setting_name = parse_args(env::args());

    let client = Client::new();
    // Use IMDSv2 for accessing instance metadata
    let uri = IMDS_SESSION_TOKEN_ENDPOINT;
    let imds_session_token = client
        .put(uri)
        .header("X-aws-ec2-metadata-token-ttl-seconds", "60")
        .send()
        .context(error::ImdsRequest { method: "PUT", uri })?
        .error_for_status()
        .context(error::ImdsResponse { uri })?
        .text()
        .context(error::ImdsText { uri })?;

    let setting = match setting_name.as_ref() {
        "cluster-dns-ip" => get_cluster_dns_ip(&client, &imds_session_token),
        "node-ip" => get_node_ip(&client, &imds_session_token),

        // If we want to specify a reasonable default in a template, we can exit 2 to tell
        // sundog to skip this setting.
        "max-pods" => get_max_pods(&client, &imds_session_token).map_err(|_| process::exit(2)),

        _ => usage(),
    }?;

    // sundog expects JSON-serialized output so that many types can be represented, allowing the
    // API model to use more accurate types.

    // 'max_pods' setting is an unsigned integer, convert 'settings' to u32 before serializing to JSON
    if setting_name == "max-pods" {
        let max_pods = serde_json::to_string(
            &setting
                .parse::<u32>()
                .context(error::ParseToU32 { setting: &setting })?,
        )
        .context(error::OutputJson { output: &setting })?;
        println!("{}", max_pods);
    } else {
        let output =
            serde_json::to_string(&setting).context(error::OutputJson { output: &setting })?;
        println!("{}", output);
    }
    Ok(())
}

// Returning a Result from main makes it print a Debug representation of the error, but with Snafu
// we have nice Display representations of the error, so we wrap "main" (run) and print any error.
// https://github.com/shepmaster/snafu/issues/110
fn main() {
    if let Err(e) = run() {
        eprintln!("{}", e);
        process::exit(1);
    }
}
