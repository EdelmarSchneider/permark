# Security Policy

Thank you for helping keep PerMark and the people who use it safe.

## Supported versions

Only the latest release of PerMark receives security fixes. If you use an older version, please update first and check whether the problem is still there. A fix comes out as a new release with a new version number. We never move or reuse the version tag of a published release.

You can find the latest release on the [Releases page](https://github.com/EdelmarSchneider/permark/releases/latest).

## How to report a vulnerability

Please report security problems privately. **Do not open a public issue**, because a public report can put other people at risk before a fix is ready.

1. Go to the [Security tab](https://github.com/EdelmarSchneider/permark/security) of this repository.
2. Click **"Report a vulnerability"**.
3. Fill in the form and submit it.

You can also go straight to the form: <https://github.com/EdelmarSchneider/permark/security/advisories/new>.

Only you and the maintainers can see this report.

## What to include

A good report helps us understand and fix the problem faster. If you can, please include:

- The PerMark version and your operating system (Windows, macOS or Linux, with its version).
- How you installed PerMark (GitHub release, Snap, Microsoft Store, winget, or built from source).
- What the problem is and what an attacker could do with it.
- Steps to reproduce it, or a small example file or proof of concept.
- Any idea you have for a fix (optional).

Please do not include real secrets, passwords or private documents. A made-up example is enough.

## What happens next

PerMark is maintained by one person, so we work on a best-effort basis. Our goal is to acknowledge your report within 7 days. After that, we will tell you whether we can confirm the problem, and we will keep you informed while we work on a fix. When the fix is released, we can credit you in the security advisory, if you wish.

## What is signed, and what is not

We want you to know exactly what you can verify:

- **The installers are not code-signed.** Your system may warn you that the app comes from an unknown developer. This is a known limit, not a sign that the file was changed.
- **Updates are signed.** PerMark's built-in updater checks the signature of each update (the `.sig` files published with every release) against a public key that is built into the app. It refuses an update whose signature does not match.

Thank you for reporting responsibly.
