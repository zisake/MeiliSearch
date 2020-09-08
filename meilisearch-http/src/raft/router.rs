use std::net::SocketAddr;

use anyhow::Result;
use async_raft::async_trait::async_trait;
use async_raft::network::RaftNetwork;
use async_raft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, ClientWriteRequest, InstallSnapshotRequest,
    InstallSnapshotResponse, VoteRequest, VoteResponse,
};
use async_raft::AppData;
use async_raft::NodeId;
use bincode::{deserialize, serialize};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use log::error;
use tokio::sync::RwLock;
use tonic::transport::channel::Channel;

use super::raft_service;
use super::raft_service::raft_service_client::RaftServiceClient;
use super::{ClientRequest, ClientResponse};

#[allow(dead_code)]
pub struct Client {
    rpc_client: RaftServiceClient<Channel>,
    addr: SocketAddr,
}

impl Client {
    pub async fn forward<D: AppData>(
        &mut self,
        req: ClientWriteRequest<D>,
    ) -> Result<ClientResponse> {
        let message = raft_service::ClientWriteRequest {
            data: serialize(&req)?,
        };
        let response = self.rpc_client.forward(message).await?;
        Ok(deserialize(&response.get_ref().data)?)
    }
}

pub struct RaftRouter {
    pub clients: DashMap<NodeId, RwLock<Client>>,
}

impl RaftRouter {
    pub fn new() -> Self {
        let clients = DashMap::new();
        Self { clients }
    }

    pub async fn add_client(&self, id: NodeId, addr: SocketAddr) -> Result<()> {
        match self.clients.entry(id) {
            Entry::Vacant(entry) => {
                let client = Client {
                    rpc_client: RaftServiceClient::connect(format!("http://{}", addr)).await?,
                    addr,
                };
                entry.insert(RwLock::new(client));
            }
            Entry::Occupied(_) => (),
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn clients(&self) -> Vec<(NodeId, String)> {
        todo!()
    }
}

#[async_trait]
impl RaftNetwork<ClientRequest> for RaftRouter {
    async fn append_entries(
        &self,
        target: NodeId,
        rpc: AppendEntriesRequest<ClientRequest>,
    ) -> Result<AppendEntriesResponse> {
        let client = self
            .clients
            .get(&target)
            .ok_or_else(|| anyhow::Error::msg(format!("Client {} not found.", target)))?;

        let payload = raft_service::AppendEntriesRequest {
            data: serialize(&rpc)?,
        };
        let mut client = client.write().await;

        match client.rpc_client.append_entries(payload).await {
            Ok(response) => {
                let response = deserialize(&response.into_inner().data)?;
                Ok(response)
            }
            Err(status) => Err(anyhow::Error::msg(status.to_string())),
        }
    }

    async fn install_snapshot(
        &self,
        target: NodeId,
        rpc: InstallSnapshotRequest,
    ) -> Result<InstallSnapshotResponse> {
        let client = self
            .clients
            .get(&target)
            .ok_or_else(|| anyhow::Error::msg(format!("Client {} not found.", target)))?;

        let payload = raft_service::InstallSnapshotRequest {
            data: serialize(&rpc)?,
        };
        let mut client = client.write().await;

        match client.rpc_client.install_snapshot(payload).await {
            Ok(response) => {
                let response = deserialize(&response.into_inner().data)?;
                Ok(response)
            }
            Err(status) => Err(anyhow::Error::msg(status.to_string())),
        }
    }

    async fn vote(&self, target: NodeId, rpc: VoteRequest) -> Result<VoteResponse> {
        let client = self
            .clients
            .get(&target)
            .ok_or_else(|| anyhow::Error::msg(format!("Client {} not found.", target)))?;

        let payload = raft_service::VoteRequest {
            data: serialize(&rpc)?,
        };
        let mut client = client.write().await;

        match client.rpc_client.vote(payload).await {
            Ok(response) => {
                let response = deserialize(&response.into_inner().data)?;
                Ok(response)
            }
            Err(status) => {
                error!("error connecting to peer: {}", status.to_string());
                Err(anyhow::Error::msg(status.to_string()))
            }
        }
    }
}