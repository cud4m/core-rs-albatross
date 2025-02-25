#!/usr/bin/env bash
# set -o errexit
# set -o nounset
# set -o pipefail

max_restarts=10
# Initializing variables
validators=()
#Array with validators PIDs
vpids=()
#Array with seed/spammer pids
spids=()
#Array with validators indexes that are killed
kindexes=()
fail=false
foldername=$(date +%Y%m%d_%H%M%S)
ERASE=false
METRICS=false
DATABASE_CLEAN=false
CONTINOUS=false
SPAMMER=false
RELEASE=false
MAX_VALIDATORS=4
RUN_ENVIRONMENT=unknown/devnet.sh
cargo="cargo run"
cargo_build="cargo build"
devnet_create="scripts/devnet/python/devnet_create.py"
tpb=150
vkill=1
down_time=10
batch_size=32
trap cleanup_exit INT

function cleanup_exit() {
    echo "Killing all validators..."
    for pid in ${vpids[@]}; do
        kill -2 $pid || true
    done
    echo "Killing seed/spammer..."
    for pid in ${spids[@]}; do
        kill -2 $pid || true
    done
    echo "Done."

    report_warnings

    if [ "$fail" = true ] ; then
        echo "...FAILED..."
        exit 1
    fi
    echo "SUCCESS" >> temp-state/RESULT.TXT
    exit 0
}

function check_failures() {
    # Periodically check for panics/crashes
    secs=0
    while [ $secs -le $sleep_time ]
    do
        # Search for panics
        if grep -rin " panic " $logsdir/*.log
        then
            echo "   !!!   PANIC   !!!"
            echo "PANIC" >> temp-state/RESULT.TXT
            fail=true
            break
        fi

        sleep 1
        secs=$(( $secs + 1 ))
    done

    if [ "$fail" = true ] ; then
        echo " Execution failed..."
        cleanup_exit
    fi
}

function report_warnings() {
    # Search for locks held for a long time
    if grep -rin "lock held for a long time" $logsdir/*.log
    then
        echo "   !!!   LONG LOCK HOLD TIME   !!!"
        echo "LONG_LOCK_HOLD_TIME" >> temp-state/RESULT.TXT
    fi

    # Search for slow lock acquisitions
    if grep -rin "slow.*took" $logsdir/*.log
    then
        echo "   !!!   SLOW LOCK ACQUISITION   !!!"
        echo "SLOW_LOCK_ACQUISITION" >> temp-state/RESULT.TXT
    fi

    # Search for deadlocks
    if grep -wrin "deadlock" $logsdir/*.log
    then
        echo "   !!!   POTENTIAL DEADLOCK DETECTED  !!!"
        echo "DEADLOCK" >> temp-state/RESULT.TXT
    fi
}

usage()
{
cat << EOF
usage: $0 [-r|--restarts COUNT] [-e|--erase] [-h|--help]

This script launches N validators and optionally restarts them while they are running

OPTIONS:
   -h|--help       Show this message
   -e|--erase      Erases all of the validator state as part of restarting it
   -m|--metrics    Enables spammer metrics (spammer must be enabled with -s).
   -d|--db         Erases only the database state of the validator as part of restarting it
   -r|--restarts   The number of times you want to kill/restart validators (by default 10 times)(0 means no restarts)
   -c|--continous  In continous mode the script runs until it is killed (or it finds an error)
   -k|--kill       How many validators are killed each cycle, by default just 1
   -s|--spammer    Launch the spammer with the given amount of transactions per second
   -R|--release    If you want to run in release mode
   -v|--validators The number of validators, as a minimum 4 validators are created
   -t|--time       Time in seconds that validators are taken down, by default 10s
   -b|--batch      Blocks per batch to properly detect the best chain, by default 32
      --run-environment Sent to Loki, like "ci", "devnet", default: "unknown/devnet.sh"
EOF
}

while [ ! $# -eq 0 ]; do
    case "$1" in
        -r | --restarts)
            if [ "$2" ]; then
                max_restarts=$2
                shift
            else
                echo '--restarts requires a value'
                exit 1
            fi
            ;;
        -s | --spammer)
            if [ "$2" ]; then
                SPAMMER=true
                tpb=$2
                shift
            else
                echo '--tpb requires a value'
                exit 1
            fi
            ;;
        -v | --validators)
            if [ "$2" ]; then
                MAX_VALIDATORS=$2
                shift
            else
                echo '--validators requires a value'
                exit 1
            fi
            ;;
        -t | --time)
            if [ "$2" ]; then
                down_time=$2
                shift
            else
                echo '--time requires a value'
                exit 1
            fi
            ;;
        -k | --kill)
            if [ "$2" ]; then
                vkill=$2
                shift
            else
                echo '--kill requires a value'
                exit 1
            fi
            ;;
        -b | --batch)
            if [ "$2" ]; then
                batch_size=$2
                shift
            else
                echo '--batch requires a value'
                exit 1
            fi
            ;;
        --run-environment)
            if [ "$2" ]; then
                RUN_ENVIRONMENT="$2"
                shift
            else
                echo '--run-environment requires a value'
                exit 1
            fi
            ;;
        -e | --erase)
            ERASE=true
            ;;
        -m | --metrics)
            METRICS=true
            ;;

        -c | --continous)
            CONTINOUS=true
            ;;
        -d | --db)
            DATABASE_CLEAN=true
            ;;
        -s | --spammmer)
            SPAMMER=true
            ;;
        -R | --release)
            RELEASE=true
            ;;
        -h | --help)
            usage
            exit
            ;;
        *)
            usage
            exit
            ;;
    esac
    shift
done

# Get git repository root and switch to it
gitroot=`git rev-parse --show-toplevel`
pushd $gitroot > /dev/null

# Create directory for logs
logsdir="temp-logs/$foldername"
configdir="$logsdir/conf"
mkdir -p $logsdir
mkdir -p $configdir

# Erase all previous state (if any) and start fresh
rm -rf temp-state/*

if [ "$RELEASE" = true ] ; then
    cargo+=" --release"
    cargo_build+=" --release"
    devnet_create+=" --release"
fi

if [ "$METRICS" = true ] ; then
    export RUSTFLAGS="--cfg tokio_unstable"
fi

echo "Number of validators: $MAX_VALIDATORS"
echo "Number of simultaneous restarts: $vkill"

if [ $MAX_VALIDATORS -lt 4 ] ; then
    echo 'min number of validators is 4'
    exit 1
fi

i=1
while  [ $i -le $MAX_VALIDATORS ]
do
    validators+=($i)
    i=$(( $i + 1 ))
done

echo "Building config files..."
devnet_command="python3 $devnet_create $MAX_VALIDATORS --run-environment $RUN_ENVIRONMENT -o $configdir"
if [ "$SPAMMER" = true ] ; then
    devnet_command="$devnet_command -s"
fi
if [ "$METRICS" = true ] ; then
    devnet_command="$devnet_command -m"
fi

# Execute the devnet command
$devnet_command

echo "Config files generated in '$configdir'"
export NIMIQ_OVERRIDE_DEVNET_CONFIG="$PWD/$configdir/dev-albatross.toml"
echo "Overriding genesis with '$NIMIQ_OVERRIDE_DEVNET_CONFIG' ..."
echo "Compiling ..."
$cargo_build
echo "Done."

# Launch the seed node
echo "Starting seed node..."
mkdir -p temp-state/dev/seed
$cargo --bin nimiq-client -- -c $configdir/seed/client.toml >> $logsdir/Seed.log 2>&1 &
spids+=($!)
sleep 3
echo "Done."

# Launch the validators and store their PID
echo "Starting validators..."
for validator in ${validators[@]}; do
    echo "    Starting Validator: $validator"
    mkdir -p temp-state/dev/$validator
    $cargo --bin nimiq-client -- -c $configdir/validator$validator/client.toml >> $logsdir/Validator$validator.log 2>&1 &
    vpids+=($!)
    sleep 1
done
echo "Done."

# Let the validators produce blocks for 30 seconds
sleep 30

# Launch the spammer
if [ "$SPAMMER" = true ] ; then
    echo "Starting spammer..."
    mkdir -p temp-state/dev/spammer
    $cargo --bin nimiq-spammer -- -t $tpb -c $configdir/spammer/client.toml >> $logsdir/Spammer.log 2>&1 &
    spids+=($!)
    sleep 1
    echo "Done."
fi

old_block_number=0
old_view_number=0
restarts_count=0

cycles=0
sim_kills=0
while [ $cycles -le $max_restarts ]
do
    if [ $restarts_count -lt $max_restarts ] ; then

        # Kill validators
        while [ $sim_kills -lt $vkill ]
        do
            # Select a random validator to restart
            index=$(($RANDOM % $MAX_VALIDATORS))

            while [[ " ${kindexes[*]} " =~ " ${index} " ]]; do
                index=$(($RANDOM % $MAX_VALIDATORS))
            done
            kindexes+=($index)
            echo "  Killing validator: $(($index + 1 ))"

            kill -2 ${vpids[$index]}
            sleep 1

            if [ "$ERASE" = true ] ; then
            echo "  Erasing all validator state"
            rm -rf temp-state/dev/$(($index + 1 ))/*
            echo "################################## VALIDATOR STATE DELETED ###########################  " >> $logsdir/Validator$(($index + 1 )).log
            fi

            if [ "$DATABASE_CLEAN" = true ] ; then
            echo "  Erasing validator database"
            rm -rf temp-state/dev/$(($index + 1 ))/devalbatross-history-consensus
            echo "################################## VALIDATOR DB DELETED ###########################  " >> $logsdir/Validator$(($index + 1 )).log
            fi

            echo "################################## RESTART ###########################  " >> $logsdir/Validator$(($index + 1 )).log
            sim_kills=$(( $sim_kills + 1 ))
        done

        # Let it run for some seconds with validators down
        if [ $vkill -gt 0 ]; then
            sleep_time=$down_time
            echo "  Running with validator(s) down for $sleep_time seconds"
            check_failures
        fi

        # Restart the ones that were killed
        for index in ${kindexes[@]}; do
            echo "  Restarting validator: $(($index + 1 ))"
            $cargo --bin nimiq-client -- -c $configdir/validator$(($index + 1 ))/client.toml >> $logsdir/Validator$(($index + 1 )).log 2>&1 &
            vpids[$index]=$!
            sleep 2
        done
        restarts_count=$(( $restarts_count + 1 ))
    fi
    # Empty kill indexes for next cycle
    kindexes=()
    sim_kills=0

    if [ "$CONTINOUS" = false ] ; then
        cycles=$(( $cycles + 1 ))
    fi

    sleep_time=$((40 + $RANDOM % 100))

    # Produce blocks for some time
    echo "  Producing blocks for $sleep_time seconds with all validators up"

    check_failures

    # Search if blocks are being produced
    bns=()

    # First collect the last block number from each validator
    for log in $logsdir/*.log; do
        bn=$({ grep "Accepted block" $log || test $? = 1; } | tail -1 | awk -F# '{print $2}' | cut -d : -f 1)
        if [ -z "$bn" ]; then
            bns+=(0.0)
        else
            bns+=($bn)
        fi
    done

    # Obtain the greatest one
    new_block_number=0
    for bn in "${bns[@]}" ; do
        block_number=$(echo $bn)
        if [ $block_number -gt $new_block_number ] ; then
            new_block_number=$block_number
        fi
    done

    echo "     Latest block: #$new_block_number"

    if [ $new_block_number -eq $old_block_number ] ; then
        echo "   !!!   BLOCKS ARE NOT BEING PRODUCED AFTER $sleep_time SECONDS   !!!"
        echo "CHAIN-STALL" >> temp-state/RESULT.TXT
        fail=true
        break
    fi
    old_block_number=$new_block_number
done

sleep 30

cleanup_exit

# Restore the original directory
popd > /dev/null
