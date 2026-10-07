# CI with GitHub Actions or Jenkins

The exported smoke script requires Bash, curl, and jq. It makes bounded retries,
respects curl's `Retry-After` handling, sets connection/request deadlines, and exits
nonzero on bad output or a failed request. CI also bounds the whole job.

Set `ELIZA_API_ROOT` to a reachable instance. When hosted fixture pinning is live,
use the published root `https://<domain>/api/fixtures/<release>` rather than the
rolling `/api` alias. That route does not exist in the current server; there is
no pinned hosted release available yet. A provider's `/v1` is not an ELIZA fixture pin.

Copy `examples/smoke.sh` into your project's `ci/eliza-smoke.sh` and keep the copy
under your normal review/version control. The workflow checks out your project,
not the private ELIZA implementation. Public access will not need a secret; set a
key only for an authenticated instance. Never run privileged secret-bearing tests
on untrusted pull-request code.

## GitHub Actions

```yaml
name: ELIZA integration
on: [push, workflow_dispatch]
permissions:
  contents: read
jobs:
  smoke:
    runs-on: ubuntu-24.04
    timeout-minutes: 2
    steps:
      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6
      - run: bash ci/eliza-smoke.sh
        env:
          ELIZA_API_ROOT: ${{ vars.ELIZA_API_ROOT }}
          ELIZA_API_KEY: ${{ secrets.ELIZA_API_KEY }}
```

## Jenkins

The agent needs Git checkout, Bash, curl, and jq. For an authenticated instance,
store the key as a Secret text credential named `eliza-api-key`:

```groovy
pipeline {
  agent any
  options { timeout(time: 2, unit: 'MINUTES') }
  environment { ELIZA_API_ROOT = credentials('eliza-api-root') }
  stages {
    stage('ELIZA integration') {
      steps {
        checkout scm
        withCredentials([string(credentialsId: 'eliza-api-key', variable: 'ELIZA_API_KEY')]) {
          sh 'set +x; bash ci/eliza-smoke.sh'
        }
      }
    }
  }
}
```

Here `eliza-api-root` is also a Secret text credential containing the instance URL;
it may instead be a plain job environment variable. For an unprotected/public
instance, remove `withCredentials` and run the same script without a key.
