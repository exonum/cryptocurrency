#!/bin/bash

function get-info {
    curl "http://127.0.0.1:8000/api/services/cryptocurrency/v1/wallets/info?pub_key=$1"
}

get-info "03e657ae71e51be60a45b4bd20bcf79ff52f0c037ae6da0540a0e0066132b472"
get-info "d1e877472a4585d515b13f52ae7bfded1ccea511816d7772cb17e1ab20830819"
