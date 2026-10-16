# 통합 테스트 전용 OpenSSH 서버. 운영 환경에서 사용하지 마세요.
FROM alpine:3.22

RUN apk add --no-cache openssh-server bash vim procps ncurses \
 && ssh-keygen -A \
 && adduser -D -s /bin/bash rivet \
 && echo 'rivet:rivet-it-password' | chpasswd \
 && mkdir -p /etc/rivet

COPY sshd_config /etc/ssh/sshd_config

EXPOSE 22
CMD ["/usr/sbin/sshd", "-D", "-e"]
