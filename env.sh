#!/bin/bash

set -eux
# 1. netns 作成
ip netns add ns1
ip netns add ns2

# 2. veth peer 作成
ip link add eth0-ns1 type veth peer name eth0-ns2

# 3. netns と veth 紐づけ
ip link set eth0-ns1 netns ns1
ip link set eth0-ns2 netns ns2

# 4. IP アドレスを追加
ip netns exec ns1 ip address add 192.168.0.101/24 dev eth0-ns1
ip netns exec ns2 ip address add 192.168.0.102/24 dev eth0-ns2

# 5. NIC 有効化
ip netns exec ns1 ip link set lo up
ip netns exec ns2 ip link set lo up
ip netns exec ns1 ip link set eth0-ns1 up
ip netns exec ns2 ip link set eth0-ns2 up

# 6. 疎通確認
ip netns exec ns1 ping 192.168.0.102 -c 3 -i 0

# 7. netns 削除
# ip -all netns delete
