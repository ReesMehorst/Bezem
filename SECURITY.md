# Security policy

Bezem deletes files, sometimes with administrator rights. We treat mistakes in what it deletes as
seriously as classic vulnerabilities.

## Project status and supported versions

Bezem is **pre-alpha**. No version has been released yet, so no version is supported for production
use. You can still report problems in the code on the `main` branch.

From the first release on, security fixes go to the **latest minor version of the current major
version**. A longer support policy for 1.x will be decided at 1.0.

## What counts as a security bug

Besides classic vulnerabilities, these are security bugs:

- **Any bug that deletes, or could delete, a file**
  - that the preview did not show,
  - outside an allowlisted root, or
  - by following a reparse point (symbolic link, junction or mount point).
- **Any network traffic** from a build that should make none. Bezem makes no network calls; the only
  exception ever considered is an opt-in update check.
- **Privilege escalation through the elevated relaunch**, for example argument injection into the
  administrator process.
- **DLL search-order hijacking** of `bezem.exe` or `bezem-gui.exe`.
- **Log injection**: content that can forge or corrupt entries in the deletion log.
- Problems in a cleaner definition that make Bezem delete user data, settings or credentials instead
  of regenerable data.

## How to report

**Please do not open a public issue.** Report privately through GitHub Private Vulnerability
Reporting:

**<https://github.com/ReesMehorst/Bezem/security/advisories/new>**

A dedicated security mailbox is not available yet. Until it is, private vulnerability reporting is
the only channel.

Please include:

- the Bezem version or commit, and the Windows version and build (`winver`);
- what happened and what you expected;
- steps to reproduce, or a proof of concept;
- for unexpected deletions: the relevant lines from the deletion log (redact your username) and
  whether the files were in the preview.

There is **no bug bounty**. We will credit you in the advisory unless you prefer not to be named.

## What happens next

These are targets for a volunteer project, not guarantees:

| Step | Target |
|---|---|
| Acknowledge your report | within 72 hours |
| Triage and severity (CVSS v4, plus a Bezem "data-loss" flag) | within 7 days |
| Fix for a data-loss bug | within 14 days |
| Fix for other issues | within 90 days |

We coordinate disclosure with you and publish a GitHub Security Advisory when a fix is available.

## If a released version deletes the wrong files

If a released version can delete files it should not, we will:

1. publish a GitHub Security Advisory and request a CVE;
2. mark the affected release "do not use" and remove it from winget and scoop;
3. pin an issue explaining who is affected and what to do;
4. ship a patch release;
5. add a regression test and write a post-mortem in `docs/incidents/`.

## If you lost data

1. **Stop writing to the disk** as far as you can. Do not install recovery tools onto the same drive.
   New writes can overwrite deleted data.
2. **Check the Recycle Bin.** Privacy items go there by default, and you may have chosen Recycle Bin
   mode for everything.
3. **Check the deletion log** in `%LOCALAPPDATA%\Bezem\logs`. It lists every path Bezem removed, so
   you know exactly what to restore.
4. **Try Windows backups**: File History, or right-click the parent folder, choose
   **Properties > Previous Versions**, and restore from there.
5. **Report it privately** using the link above, even if you recovered the files. It helps us find
   and fix the cause.
