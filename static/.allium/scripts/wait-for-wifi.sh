#!/bin/sh
# Waits up to a minute for wlan0 to have an IPv4 address.
#
# This used to ping 1.1.1.1, which asks the wrong question. Every service it gates -- the file
# server, FTP, SSH, telnet, syncthing -- is a LAN service, and none needs the internet. On a network
# with no route out (a firewall, a guest network, a router with the WAN unplugged) the ping never
# succeeded, so none of them could ever start, while the toggles that started them still read On.
# An address on the interface is what association plus DHCP actually produce, and it is all a LAN
# service needs. NTP still needs the internet, and simply fails when there is none, as it did.

counter=0
while ! ip addr show dev wlan0 2>/dev/null | grep -q 'inet '; do
	counter=$((counter+1))
	if [ $counter -gt 60 ]; then
		exit 1
	fi
	sleep 1
done

exit 0
