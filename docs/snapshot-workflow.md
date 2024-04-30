# Snapshot Workflow

Sharing Subgraph snapshot is the insipration that led to the creation of Files Service. In this document we describe the workflows for producers and consumers of Subgraph snapshots in detail.


## Producer taking Subgraph snapshot

Data producer owns a database that contains a various amount of indexed Subgraphs. They want to share a particular Subgraph Deployment from the database. We assume that they are already running a file server ([guide](/docs/server_guide.md)).

Thus they set Postgres environmental variables `POSTGRES_DB_HOST, POSTGRES_DB_PORT, POSTGRES_DB_NAME, POSTGRES_DB_USER,  POSTGRES_DB_PSWD`, and the intented Subgraph Deployment `SNAPSHOT_SUBGRAPH_DEPLOYMENT_IPFS_HASH`. Then, they will run the script we provided at `scripts/take-snapshot.sh`, which will check for the existence of intended deployment, take `pg_dump` of the corresponding schema into a file locally, and print out a line of metadata. 

The script also takes environmental variable `SERVER_ADMIN_ENDPOINT` and `AUTH_TOKEN` to automatically publish and add the snapshot as part of their file service. The producer can also manually go to their file service admin endpoint to publish and serve the snapshot; they should include the snapshot file, and the metadata printed out in the description.

## Consumer downloading and loading Subgraph Snapshot

Data consumer similarly has a graph-node database. They are syncing a Subgraph Deployment, but decided it would be better to simply buy the latest snapshot from a data producer which reduces the consumer's computation cost and opportunity cost to serve queries. 

Here we assume that the Data consumer knows the public endpoints of data producers. They ping the endpoints for all available files and identifies the manifest hash for the Subgraph Snapshot they would like to download. 

They use the downloader to download the file. Refer to [Downloader](/docs/client_guide.md) for more details.

Once the file is downloaded, the data consumer may use the script provided at `scripts/load_snapshot.sh`. This script will check for existence of the locally syncing Subgraph schema, delete the local copy, modify the remote snapshot to use local schema identifier and owner, load in the localized snapshot, and update the metadata as provided in the manifest.

To confirm everything is working, visit graph-node's index node server to check the indexing statuses and the subgraph query endpoint to test the queries.


