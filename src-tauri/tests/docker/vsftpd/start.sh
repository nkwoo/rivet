#!/bin/bash
# 두 vsftpd 인스턴스를 모두 셸의 자식으로 실행합니다. 한쪽을 exec로 PID 1로 만들면 그 vsftpd가
# 다른 인스턴스를 자식으로 넘겨받고, 모르는 자식의 종료 신호(SIGCHLD)를 받아 비정상 종료합니다.
/usr/sbin/vsftpd /etc/vsftpd/vsftpd-implicit.conf &
/usr/sbin/vsftpd /etc/vsftpd/vsftpd.conf &
wait -n
