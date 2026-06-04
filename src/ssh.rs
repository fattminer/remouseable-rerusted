// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

use bytes::Bytes;
use russh::{
    ChannelMsg, Disconnect, client,
    keys::{
        self,
        agent::{AgentIdentity, client::AgentStream},
        ssh_key,
    },
};
use std::{
    io::{self, Read},
    path::PathBuf,
    sync::{Arc, mpsc},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SshOptions {
    pub address: String,
    pub user: String,
    pub password: String,
    pub agent_socket: String,
    pub event_file: String,
    pub known_hosts: Option<PathBuf>,
}

struct Client {
    host: String,
    port: u16,
    known_hosts: Option<PathBuf>,
}

impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        let Some(path) = &self.known_hosts else {
            return Ok(true);
        };
        keys::check_known_hosts_path(&self.host, self.port, server_public_key, path)
            .map_err(russh::Error::Keys)
    }
}

pub struct SshEventReader {
    _runtime: tokio::runtime::Runtime,
    receiver: mpsc::Receiver<Result<Bytes, String>>,
    current: Bytes,
    offset: usize,
}

impl Read for SshEventReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }

        while self.offset >= self.current.len() {
            match self.receiver.recv() {
                Ok(Ok(bytes)) if bytes.is_empty() => {}
                Ok(Ok(bytes)) => {
                    self.current = bytes;
                    self.offset = 0;
                }
                Ok(Err(error)) => return Err(io::Error::other(error)),
                Err(_) => return Ok(0),
            }
        }

        let available = &self.current[self.offset..];
        let length = output.len().min(available.len());
        output[..length].copy_from_slice(&available[..length]);
        self.offset += length;
        Ok(length)
    }
}

/// Opens a blocking reader backed by a live SSH event stream.
///
/// # Errors
///
/// Returns an error when parameters are invalid, the connection fails,
/// authentication fails, or the remote event command cannot start.
pub fn open_event_stream(options: &SshOptions) -> io::Result<SshEventReader> {
    validate_event_file(&options.event_file)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let (sender, receiver) = mpsc::channel();
    runtime.block_on(connect_and_spawn(options.clone(), sender))?;
    Ok(SshEventReader {
        _runtime: runtime,
        receiver,
        current: Bytes::new(),
        offset: 0,
    })
}

async fn connect_and_spawn(
    options: SshOptions,
    sender: mpsc::Sender<Result<Bytes, String>>,
) -> io::Result<()> {
    let (host, port) = parse_address(&options.address)?;
    let handler = Client {
        host,
        port,
        known_hosts: options.known_hosts,
    };
    let config = Arc::new(client::Config {
        nodelay: true,
        ..client::Config::default()
    });
    let mut session = client::connect(config, options.address.as_str(), handler)
        .await
        .map_err(ssh_error)?;

    let authenticated = if options.password.is_empty() {
        authenticate_agent(&mut session, &options.user, &options.agent_socket).await?
    } else {
        session
            .authenticate_password(&options.user, options.password)
            .await
            .map_err(ssh_error)?
            .success()
    };
    if !authenticated {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "SSH authentication failed",
        ));
    }

    let mut channel = session.channel_open_session().await.map_err(ssh_error)?;
    channel
        .exec(true, event_command(&options.event_file))
        .await
        .map_err(ssh_error)?;

    tokio::spawn(async move {
        let mut exit_status = None;
        let mut stderr = Vec::new();
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Data { data } => {
                    if sender.send(Ok(data)).is_err() {
                        break;
                    }
                }
                ChannelMsg::ExtendedData { data, .. } => stderr.extend_from_slice(&data),
                ChannelMsg::ExitStatus {
                    exit_status: status,
                } => exit_status = Some(status),
                _ => {}
            }
        }
        if exit_status.is_some_and(|status| status != 0) {
            let message = String::from_utf8_lossy(&stderr);
            let _ = sender.send(Err(format!(
                "remote event command failed with status {}: {}",
                exit_status.unwrap_or_default(),
                message.trim()
            )));
        }
        let _ = session
            .disconnect(Disconnect::ByApplication, "", "English")
            .await;
    });
    Ok(())
}

#[cfg(unix)]
async fn authenticate_agent(
    session: &mut client::Handle<Client>,
    user: &str,
    socket: &str,
) -> io::Result<bool> {
    if socket.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--ssh-socket or SSH_AUTH_SOCK is required for agent authentication",
        ));
    }
    let mut agent = keys::agent::client::AgentClient::connect_uds(socket)
        .await
        .map_err(agent_error)?;
    authenticate_agent_identities(session, user, &mut agent).await
}

#[cfg(windows)]
async fn authenticate_agent(
    session: &mut client::Handle<Client>,
    user: &str,
    socket: &str,
) -> io::Result<bool> {
    let socket = if socket.is_empty() {
        r"\\.\pipe\openssh-ssh-agent"
    } else {
        socket
    };
    let mut agent = keys::agent::client::AgentClient::connect_named_pipe(socket)
        .await
        .map_err(agent_error)?;
    authenticate_agent_identities(session, user, &mut agent).await
}

async fn authenticate_agent_identities<S>(
    session: &mut client::Handle<Client>,
    user: &str,
    agent: &mut keys::agent::client::AgentClient<S>,
) -> io::Result<bool>
where
    S: AgentStream + Unpin + Send + 'static,
{
    let identities = agent.request_identities().await.map_err(agent_error)?;
    for identity in identities {
        if let AgentIdentity::PublicKey { key, .. } = identity {
            let result = session
                .authenticate_publickey_with(user, key, None, agent)
                .await
                .map_err(agent_auth_error)?;
            if result.success() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn ssh_error(error: russh::Error) -> io::Error {
    io::Error::other(error)
}

fn agent_error(error: keys::Error) -> io::Error {
    io::Error::other(error)
}

fn agent_auth_error(error: russh::AgentAuthError) -> io::Error {
    io::Error::other(error)
}

fn parse_address(address: &str) -> io::Result<(String, u16)> {
    if let Some(remainder) = address.strip_prefix('[') {
        let Some((host, port)) = remainder.rsplit_once("]:") else {
            return Err(invalid_address(address));
        };
        return Ok((host.to_owned(), parse_port(address, port)?));
    }

    let Some((host, port)) = address.rsplit_once(':') else {
        return Err(invalid_address(address));
    };
    if host.is_empty() {
        return Err(invalid_address(address));
    }
    Ok((host.to_owned(), parse_port(address, port)?))
}

fn parse_port(address: &str, port: &str) -> io::Result<u16> {
    port.parse().map_err(|_| invalid_address(address))
}

fn invalid_address(address: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("invalid SSH address {address:?}; expected host:port"),
    )
}

fn validate_event_file(path: &str) -> io::Result<()> {
    if path.starts_with('/')
        && path.len() > 1
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'-' | b'.'))
    {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "event file must be an absolute path containing only letters, numbers, '/', '_', '-', and '.'",
    ))
}

fn event_command(path: &str) -> String {
    format!("cat '{path}'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_original_event_paths() {
        assert!(validate_event_file("/dev/input/event0").is_ok());
        assert!(validate_event_file("/dev/input/event1").is_ok());
        assert_eq!(
            event_command("/dev/input/event1"),
            "cat '/dev/input/event1'"
        );
    }

    #[test]
    fn rejects_shell_injection_in_event_path() {
        assert!(validate_event_file("/dev/input/event0; reboot").is_err());
        assert!(validate_event_file("$(reboot)").is_err());
        assert!(validate_event_file("relative/event0").is_err());
    }

    #[test]
    fn parses_original_ssh_addresses() {
        assert_eq!(
            parse_address("10.11.99.1:22").unwrap(),
            ("10.11.99.1".to_owned(), 22)
        );
        assert_eq!(
            parse_address("remarkable.local:2222").unwrap(),
            ("remarkable.local".to_owned(), 2222)
        );
        assert_eq!(parse_address("[::1]:22").unwrap(), ("::1".to_owned(), 22));
    }
}
