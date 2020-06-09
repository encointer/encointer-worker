#!/bin/bash

# setup:
# run all on localhost:
#   encointer-node purge-chain --dev
#   encointer-node --dev --ws-port 9979 -lruntime=debug
#   rm chain_relay_db.bin
#   encointer-worker init-shard 3LjCHdiNbNLKEtwGtBf6qHGZnfKFyjLu9v3uxVgDL35C
#   encointer-worker shielding-key
#   encointer-worker signing-key
#   encointer-worker -p 9979 -w 2079 run 3LjCHdiNbNLKEtwGtBf6qHGZnfKFyjLu9v3uxVgDL35C
#
# then run this script

# usage:
#  demo_shielding_unshielding.sh <NODEPORT> <WORKERPORT>

# using default port if none given as first argument
NPORT=${1:-9944}
WPORT=${2:-2000}

echo "Using node-port ${NPORT}"
echo "Using worker-port ${WPORT}"
echo ""

CLIENT="../target/release/encointer-client -p ${NPORT} "
WORKERPORT="--worker-port ${WPORT}"
SHARD="3LjCHdiNbNLKEtwGtBf6qHGZnfKFyjLu9v3uxVgDL35C"

# register new currency
cid=$($CLIENT new-currency test-locations-mediterranean.json //Alice)
echo $cid

# list currenies
$CLIENT list-currencies

# bootstrap currency with well-known keys
phase=$($CLIENT get-phase)
echo "phase is $phase"
if [ "$phase" == "REGISTERING" ]; then
   echo "that's fine"
elif [ "$phase" == "ASSIGNING" ]; then
   echo "need to advance"
   $CLIENT next-phase   
   $CLIENT next-phase
elif [ "$phase" == "ATTESTING" ]; then
   echo "need to advance"
   $CLIENT next-phase   
fi
phase=$($CLIENT get-phase)
echo "phase is now: $phase"

read MRENCLAVE <<< $(${CLIENT} list-workers | awk '/  MRENCLAVE: / { print $2 }')
echo "  MRENCLAVE = ${MRENCLAVE}"

# new account with
# $CLIENT trusted new-account --mrenclave $MRENCLAVE --shard $SHARD

account1=//Alice
account2=//Bob
account3=//Charlie

$CLIENT trusted get-registration $account1 --mrenclave $MRENCLAVE --shard $cid $WORKERPORT
# should be zero

$CLIENT trusted register-participant $account1 --mrenclave $MRENCLAVE --shard $cid $WORKERPORT
$CLIENT trusted register-participant $account2 --mrenclave $MRENCLAVE --shard $cid $WORKERPORT
$CLIENT trusted register-participant $account3 --mrenclave $MRENCLAVE --shard $cid $WORKERPORT

echo "Registered Participants"

# should be 1,2 and 3
$CLIENT trusted get-registration $account1 --mrenclave $MRENCLAVE --shard $cid $WORKERPORT
$CLIENT trusted get-registration $account2 --mrenclave $MRENCLAVE --shard $cid $WORKERPORT
$CLIENT trusted get-registration $account3 --mrenclave $MRENCLAVE --shard $cid $WORKERPORT

$CLIENT next-phase
# should now be ASSIGNING

#$CLIENT --cid $cid list-meetup-registry

$CLIENT next-phase
# should now be ATTESTING

echo "* Waiting 5 seconds such that phase change happened in enclave"
sleep 5
echo ""

echo "*** start meetup"
claim1=$($CLIENT trusted new-claim $account1 3 --mrenclave $MRENCLAVE --shard $cid $WORKERPORT)
claim2=$($CLIENT trusted new-claim $account2 3 --mrenclave $MRENCLAVE --shard $cid $WORKERPORT)
claim3=$($CLIENT trusted new-claim $account3 3 --mrenclave $MRENCLAVE --shard $cid $WORKERPORT)

echo "Claim1 = ${claim1}"
echo "Claim1 = ${claim2}"
echo "Claim1 = ${claim3}"

echo "*** sign each others claims"
witness1_2=$($CLIENT sign-claim $account1 $claim2)
witness1_3=$($CLIENT sign-claim $account1 $claim3)

witness2_1=$($CLIENT sign-claim $account2 $claim1)
witness2_3=$($CLIENT sign-claim $account2 $claim3)

witness3_1=$($CLIENT sign-claim $account3 $claim1)
witness3_2=$($CLIENT sign-claim $account3 $claim2)

echo "*** send witnesses to chain"
$CLIENT trusted register-attestations $account1 $witness2_1 $witness3_1 --mrenclave $MRENCLAVE --shard $cid $WORKERPORT
$CLIENT trusted register-attestations $account2 $witness1_2 $witness3_2 --mrenclave $MRENCLAVE --shard $cid $WORKERPORT
$CLIENT trusted register-attestations $account3 $witness1_3 $witness2_3 --mrenclave $MRENCLAVE --shard $cid $WORKERPORT


$CLIENT trusted get-attestations $account1 --mrenclave $MRENCLAVE --shard $cid $WORKERPORT
$CLIENT trusted get-attestations $account2 --mrenclave $MRENCLAVE --shard $cid $WORKERPORT
$CLIENT trusted get-attestations $account3 --mrenclave $MRENCLAVE --shard $cid $WORKERPORT


$CLIENT next-phase
# should now be REGISTERING

echo "account balances for new currency with cid $cid"
$CLIENT trusted balance $account1 ${WORKERPORT} --mrenclave $MRENCLAVE --shard $cid
$CLIENT trusted balance $account2 ${WORKERPORT} --mrenclave $MRENCLAVE --shard $cid
$CLIENT trusted balance $account3 ${WORKERPORT} --mrenclave $MRENCLAVE --shard $cid