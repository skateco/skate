import http from 'k6/http';
import { check } from 'k6';
import { sleep } from 'k6';

export const options = {
  scenarios: {
    constant_request_rate: {
      executor: 'constant-arrival-rate',
      rate: 5,
      timeUnit: '1s', // 1000 iterations per second, i.e. 1000 RPS
      duration: '60s',
      preAllocatedVUs: 10, // how large the initial pool of VUs would be
      maxVUs: 20, // if the preAllocatedVUs are not enough, we can initialize more
    },
  },
};
export default function () {
  const res = http.get('https://byrnedo.com');
  check(res, {
    '200': (r) => r.status == 200,
    '422': (r) => r.status == 422,
    '423': (r) => r.status == 422,
    '500': (r) => r.status == 500,
    '502': (r) => r.status == 502,
    '503': (r) => r.status == 503,
    'no response': (r) => !r.status,
  })
  sleep(1);
}
