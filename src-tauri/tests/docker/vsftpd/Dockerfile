# 통합 테스트 전용 vsftpd 서버. 운영 환경에서 사용하지 마세요.
# 포트 21: 평문 FTP + 명시적 FTPS(AUTH TLS), 포트 990: 암시적 FTPS. 인증서는 자체 서명입니다.
FROM alpine:3.22

RUN apk add --no-cache vsftpd openssl bash \
 && adduser -D -s /bin/bash rivet \
 && echo 'rivet:rivet-it-password' | chpasswd \
 && mkdir -p /etc/vsftpd /var/run/vsftpd/empty \
 && openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=rivet-it" \
      -keyout /etc/vsftpd/key.pem -out /etc/vsftpd/cert.pem 2>/dev/null

COPY vsftpd.conf vsftpd-implicit.conf start.sh /etc/vsftpd/
RUN chmod +x /etc/vsftpd/start.sh

EXPOSE 21 990 21100-21119
CMD ["/etc/vsftpd/start.sh"]
