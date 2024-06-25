import http from 'k6/http';
import { check } from 'k6';

export const options = {
  tlsCipherSuites: ['TLS_RSA_WITH_RC4_128_SHA', 'TLS_RSA_WITH_AES_128_GCM_SHA256'],
  tlsVersion: {
    min: 'tls1.1',
    max: 'tls1.2',
  },
};

// ./k6 run --insecure-skip-tls-verify --vus 3 -d 10s tests/k6-tls-test.js

export default function () {
  const res = http.get('https://www.test1.com:8080');
  check(res, {
    'is TLSv1.2': (r) => r.tls_version === http.TLS_1_2,
    'is sha256 cipher suite': (r) => r.tls_cipher_suite === 'TLS_RSA_WITH_AES_128_GCM_SHA256',
  });
}
