#!/bin/bash
set -uex

apt install -y ethtool

ip netns add labo1
ip netns add labo2

ip link add labo1-eth netns labo1 type veth peer labo2-eth netns labo2

ip netns exec labo1 ip address add 192.168.100.1/24 dev labo1-eth
ip netns exec labo2 ip address add 192.168.100.2/24 dev labo2-eth

ip netns exec labo1 ip link set lo up
ip netns exec labo1 ip link set labo1-eth up
ip netns exec labo2 ip link set lo up
ip netns exec labo2 ip link set labo2-eth up

# drop RST
ip netns exec labo1 iptables -A OUTPUT -p tcp --tcp-flags RST RST -j DROP
ip netns exec labo2 iptables -A OUTPUT -p tcp --tcp-flags RST RST -j DROP

# turn off checksum offloading
ip netns exec labo1 ethtool -K labo1-eth tx off
ip netns exec labo2 ethtool -K labo2-eth tx off
