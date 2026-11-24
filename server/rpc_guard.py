"""Ограничиваем управляющий TCP-порт движка локальным сервисным пользователем и root."""
import pwd
import subprocess


def main():
    uid = pwd.getpwnam("uplink-net").pw_uid
    nft = "/usr/sbin/nft"
    # Все изменения применяются одной транзакцией ядра; при рестарте правила остаются активны.
    rules = f"""add table inet uplink_rpc_guard
flush table inet uplink_rpc_guard
add chain inet uplink_rpc_guard output {{ type filter hook output priority -10; policy accept; }}
add rule inet uplink_rpc_guard output ip daddr 127.0.0.1 tcp dport 941 meta skuid != {{ 0, {uid} }} reject
add rule inet uplink_rpc_guard output ip saddr 127.0.0.1 tcp sport 941 meta skuid != {{ 0, {uid} }} reject
add chain inet uplink_rpc_guard input {{ type filter hook input priority -10; policy accept; }}
add rule inet uplink_rpc_guard input iifname != "lo" ip daddr 127.0.0.1 tcp dport 941 drop
"""
    subprocess.run([nft, "-f", "-"], input=rules.encode(), check=True, timeout=10)


if __name__ == "__main__":
    main()

