#!/bin/bash

source ../.env

# ASSUME file service is running
DB_HOST=${POSTGRES_DB_HOST}
DB_PORT=${POSTGRES_DB_PORT}
DB_NAME=${POSTGRES_DB_NAME}
DB_USER=${POSTGRES_DB_USER}
DB_PSWD=${POSTGRES_DB_PSWD}
IPFS_HASH=${SNAPSHOT_SUBGRAPH_DEPLOYMENT_IPFS_HASH}
SERVER_ADMIN_ENDPOINT=${SERVER_ADMIN_ENDPOINT}
AUTH_TOKEN=${AUTH_TOKEN}
SERVER_STORE=${SERVER_STORE}

DEPLOYMENT_SCHEMA_QUERY="SELECT * FROM deployment_schemas WHERE subgraph = '${IPFS_HASH}';"
DEPLOYMENT_SCHEMA_RESULT=$(PGPASSWORD=$DB_PSWD psql -h ${DB_HOST} -p ${DB_PORT} -U ${DB_USER} -d ${DB_NAME} -t -A -c "${DEPLOYMENT_SCHEMA_QUERY}")

echo ${DEPLOYMENT_SCHEMA_RESULT}
if [ -n "${DEPLOYMENT_SCHEMA_RESULT}" ]; then
echo "Deployment found in the database!"
else
echo "Deployment schema not found in the database. Make sure you have this deployment on graph-node"
exit
fi

# #  id |                    subgraph                    | name  | version |  shard  | network | active |          created_at           
# # ----+------------------------------------------------+-------+---------+---------+---------+--------+-------------------------------
# #  10 | QmRAbgoZ2mBpxqj4Z32KFaso8rsDhdm8KcwEQhWMzD8bTN | sgd10 |       1 | primary | mainnet | t      | 2024-04-25 13:33:31.955611-07
# # (1 row)

IFS='|' read -r -a array <<< "${DEPLOYMENT_SCHEMA_RESULT}"
# CHECK SCHEMA id sgdNNN
schema_id=${array[0]}
sgdNNN=${array[2]}
echo "ID: ${schema_id}"
echo "sgdNNN: ${sgdNNN}"


# Take the snapshot using pg_dump
FILE_NAME="snapshot_${IPFS_HASH}.sql"
SNAPSHOT_ACTION=$(PGPASSWORD=$DB_PSWD pg_dump -h $DB_HOST -U $DB_USER -d $DB_NAME -n $sgdNNN > ${SERVER_STORE}/${FILE_NAME})
echo "Action result: ${SNAPSHOT_ACTION}"
echo "Snapshot created: ${FILE_NAME}"
# Normalize the snapshot file to use placeholder identifier and owner
sed -i -E "s/sgd[0-9]+/sgdNNN/g" ${SERVER_STORE}/${FILE_NAME}
sed -i "s/OWNER TO [^;]*;/OWNER TO graphuser;/g" ${SERVER_STORE}/${FILE_NAME}

# Query metadata data
METADATA_QUERY="SELECT deployment, failed, synced, latest_ethereum_block_hash, latest_ethereum_block_number, entity_count, graft_base, graft_block_hash, graft_block_number, fatal_error, non_fatal_errors, health, reorg_count, current_reorg_depth, max_reorg_depth, last_healthy_ethereum_block_hash, last_healthy_ethereum_block_number, id, firehose_cursor, debug_fork, earliest_block_number FROM subgraphs.subgraph_deployment WHERE deployment = '${IPFS_HASH}';"
METADATA_RESULT=$(PGPASSWORD=$DB_PSWD psql -h ${DB_HOST} -p ${DB_PORT} -U ${DB_USER} -d ${DB_NAME} -t -A -c "${METADATA_QUERY}")
echo "Metadata result: ${METADATA_RESULT}"

# Publish to Indexer file service
# Files: snapshot_IPFS_HASH.sql
# With metadata in description
IFS='|' read -r -a array <<< "${METADATA_RESULT}"
deployment=${array[0]}
failed=${array[1]}
synced=${array[2]}
temp_hash = ${array[3]}
latest_ethereum_block_hash=$(echo "${temp_hash}" | sed 's/\\/0/g')
latest_ethereum_block_number=${array[4]}
entity_count=${array[5]}
graft_base=${array[6]}
graft_block_hash=${array[7]}
graft_block_number=${array[8]}
fatal_error=${array[9]}
non_fatal_errors=${array[10]}
health=${array[11]}
reorg_count=${array[12]}
current_reorg_depth=${array[13]}
max_reorg_depth=${array[14]}
last_healthy_ethereum_block_hash=${array[15]}
last_healthy_ethereum_block_number=${array[16]}
id=${array[17]}
firehose_cursor=${array[18]}
debug_fork=${array[19]}
earliest_block_number=${array[20]}

latest_ethereum_block_hash="${array[3]//\\x/0x}"
description="""$deployment|$failed|$synced|$latest_ethereum_block_hash|$latest_ethereum_block_number|$entity_count|$graft_base|$graft_block_hash|$graft_block_number|$fatal_error|$non_fatal_errors|$health|$reorg_count|$current_reorg_depth|$max_reorg_depth|$last_healthy_ethereum_block_hash|$last_healthy_ethereum_block_number|$id|$firehose_cursor|$debug_fork|$earliest_block_number"""
echo "\n"
echo "DescriptioN: ${description}"

GRAPHQL_QUERY="""mutation{publishAndServeBundle(filenames:[\\\"${FILE_NAME}\\\"], description:\\\"${description}\\\"){ipfsHash}}"""

echo "Response: "  
curl -X POST -H "Content-Type: application/json" -H "Authorization: Bearer ${AUTH_TOKEN}" -d "{\"query\": \"${GRAPHQL_QUERY}\"}" ${SERVER_ADMIN_ENDPOINT}

# Some error handling (exit script on failure)
if [ $? -ne 0 ]; then
  echo "Error: Script failed to execute."
  exit 1
fi

echo "Script completed successfully."
