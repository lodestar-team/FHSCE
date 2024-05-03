#!/bin/bash

source .env

DB_HOST=${POSTGRES_DB_HOST}
DB_PORT=${POSTGRES_DB_PORT}
DB_NAME=${POSTGRES_DB_NAME}
DB_USER=${POSTGRES_DB_USER}
DB_PSWD=${POSTGRES_DB_PSWD}
IPFS_HASH=${SNAPSHOT_SUBGRAPH_DEPLOYMENT_IPFS_HASH}
SNAPSHOT_FILE=${SNAPSHOT_SUBGRAPH_DEPLOYMENT_FILE}
MANIFEST_HASH=${SNAPSHOT_SUBGRAPH_MANIFEST_IPFS_HASH}

DEPLOYMENT_SCHEMA_QUERY="SELECT * FROM deployment_schemas WHERE subgraph = '${IPFS_HASH}';"
DEPLOYMENT_SCHEMA_RESULT=$(PGPASSWORD=$DB_PSWD psql -h ${DB_HOST} -p ${DB_PORT} -U ${DB_USER} -d ${DB_NAME} -t -A -c "${DEPLOYMENT_SCHEMA_QUERY}")
# Ensure that graph-node already has the metadata skeleton is not empty
echo "First check for schema skeleton"
echo ${DEPLOYMENT_SCHEMA_RESULT}
if [ -n "${DEPLOYMENT_SCHEMA_RESULT}" ]; then
echo "Depolyment found in the database!"
else
echo "Deployment schema not found in the database. Make sure to first deploy on graph-node"
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

# Customize the snapshot file to use the identifier assigned locally; and modifies the owner
sed -i "s/sgd\([0-9]\+\|NNN\)/${sgdNNN}/g" ${SNAPSHOT_FILE}
sed -i "s/OWNER TO [^;]*;/OWNER TO ${DB_USER};/g" ${SNAPSHOT_FILE}

# Drop local schema and load in remote
echo "Deleting local subgraph schema"
DROP_SCHEMA_QUERY="DROP SCHEMA IF EXISTS ${array[2]} CASCADE;"
DELETE_LOCAL_SCHEMA_RESULT=$(PGPASSWORD=$DB_PSWD psql -h ${DB_HOST} -p ${DB_PORT} -U ${DB_USER} -d ${DB_NAME} -t -A -c "${DROP_SCHEMA_QUERY}")
echo "Delete result: ${DELETE_LOCAL_RESULT}"
echo "Loading subgraph snapshot"
REMOTE_LOAD_RESULT=$(PGPASSWORD=$DB_PSWD psql -h $DB_HOST -p $DB_PORT -d $DB_NAME -U $DB_USER -f $SNAPSHOT_FILE)
echo "Load result: ${REMOTE_LOAD_RESULT}"

# Update metadata
response=$(curl -s "https://ipfs.network.thegraph.com/ipfs/api/v0/cat?arg=${MANIFEST_HASH}")
echo "Snapshot metadata: ${response}"
description=$(echo "$response" | grep 'description:' | cut -d ':' -f 2- | tr -d ' ')
IFS='|' read -r -a fields <<< "$description"

convert_types() {
    for i in "${!fields[@]}"; do
        if [ -z "${fields[$i]}" ]; then
            fields[$i]="NULL"
        elif [ "${fields[$i]}" == "{}" ]; then
            # Handle the non_fatal_errors field
            if [ $i -eq 10 ]; then  # Assuming index 10 is non_fatal_errors
                fields[$i]="ARRAY[]::text[]"
            else
                fields[$i]="NULL"
            fi
        elif [[ "${fields[$i]}" =~ ^\{.*\}$ ]]; then
            # Handle non_empty non_fatal_errors
            if [ $i -eq 10 ]; then
                # Trim leading and trailing braces and convert to PostgreSQL array format
                local clean_errors="${fields[$i]#\{}"   # Remove leading {
                clean_errors="${clean_errors%\}}"       # Remove trailing }
                fields[$i]="ARRAY['${clean_errors//,/','}']::text[]"
            fi
        else
            # Convert 'f' and 't' to FALSE and TRUE
            case "${fields[$i]}" in
                f) fields[$i]="FALSE" ;;
                t) fields[$i]="TRUE" ;;
            esac
        fi
    done
}
convert_types

# Construct the SQL UPDATE statement
METADATA_MUTATION="UPDATE subgraphs.subgraph_deployment SET
    failed = ${fields[1]},
    synced = ${fields[2]},
    latest_ethereum_block_hash = '${fields[3]}',
    latest_ethereum_block_number = ${fields[4]},
    entity_count = ${fields[5]},
    graft_base = ${fields[6]},
    graft_block_hash = ${fields[7]},
    graft_block_number = ${fields[8]},
    fatal_error = ${fields[9]},
    non_fatal_errors = ${fields[10]},
    health = '${fields[11]}',
    reorg_count = ${fields[12]},
    current_reorg_depth = ${fields[13]},
    max_reorg_depth = ${fields[14]},
    last_healthy_ethereum_block_hash = ${fields[15]},
    last_healthy_ethereum_block_number = ${fields[16]},
    id = ${schema_id},
    firehose_cursor = ${fields[18]},
    debug_fork = ${fields[19]},
    earliest_block_number = ${fields[20]}
WHERE deployment = '${fields[0]}';"

echo "------- created mutation statement ---------"
echo ${METADATA_MUTATION}

# Apply metadata changes
METADATA_MUTATION_RESULT=$(PGPASSWORD=$DB_PSWD psql -h $DB_HOST -p $DB_PORT -U $DB_USER -d $DB_NAME <<< "$METADATA_MUTATION")
echo "Mutation result: ${METADATA_MUTATION_RESULT}"

# Some error handling (exit script on failure)
if [ $? -ne 0 ]; then
  echo "Error: Script failed to execute SQL statements."
  exit 1
fi

echo "Script completed successfully."
