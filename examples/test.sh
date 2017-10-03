#!/bin/bash

function send-transaction {
    curl -H "Content-Type: application/json" -X $2 -d @$1 http://127.0.0.1:8000/api/services/cryptocurrency/v1/$3
}

send-transaction create-wallet-1.json POST wallets
send-transaction create-wallet-2.json POST wallets
send-transaction transfer-funds.json POST wallets/transfer
