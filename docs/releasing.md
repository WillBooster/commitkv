# Releasing

Register npm and PyPI Trusted Publishers for `release.yml` in `WillBooster/commitkv`,
matching the environments used by the publication jobs. The npm publisher must allow
`npm publish`. PyPI uses the `pypi` environment.

## Publish npm locally

Use a release workflow run whose native builds and npm consumer checks passed. Run from
`main` at that run's commit, then download the staged package before starting the release:

```bash
gh run list --workflow release.yml --branch main --json databaseId,headSha,status,conclusion
gh run download <run-id> -n npm-release-package -D .tmp/npm-package
npm login --registry=https://registry.npmjs.org/
GITHUB_REPOSITORY=WillBooster/commitkv GITHUB_TOKEN="$(gh auth token)" bun wb release -- --no-ci
```

The staged package and the checkout must come from the same commit. Local publication uses
npm's authenticated account, including OTP authentication when required. After completing a
local publication, rerun failed jobs of that workflow run to complete Python publication.

## Retry PyPI publication

Dispatch the workflow on the existing release tag:

```bash
gh workflow run release.yml --ref v1.0.0
```

Use the tag of the version to retry. The source commit must be in main history and match that
version's npm package. The workflow tests rebuilt Python distributions and skips files already
uploaded to PyPI.
