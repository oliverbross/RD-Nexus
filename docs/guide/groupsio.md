# Groups.io

Groups.io is an installed-app workspace for reading group traffic, keeping a
bounded offline archive, preparing drafts and posting only after an explicit
confirmation. It appears immediately after **Memories** in the main navigation.
The browser-only Remote UI does not expose the workspace because it cannot use
the computer's credential vault.

## Connect an account

Open **Settings → Groups.io**, enter the account name and a personal Groups.io
API key, then choose **Connect and verify**. Create and manage API keys at
Groups.io; do not enter the account password. RD-Nexus verifies the key before
storing it and keeps the secret in macOS Keychain, Windows Credential Manager,
or Linux Secret Service. It is not written into the ordinary settings or cache.

After connecting, **Sync now** downloads the memberships visible to that key.
Each membership can inherit the global policy or override whether it is
included, how far back to fetch, how many messages to retain, whether muted
topics are included, and whether attachments are downloaded automatically.
Opening the Groups.io section can refresh memberships when **Refresh on
opening Groups.io** is enabled; there is no hidden background polling while the
screen is closed.

## Read and search

The workspace has three linked panes: groups, topics and messages. Choosing a
group loads its cached topics; choosing a topic opens the chronological thread.
**Older topics** and **More messages** advance the provider's pagination without
discarding the pages already cached.

Search is local by default, so downloaded subjects, senders and message bodies
remain searchable while offline. The official archive action can fetch the
provider's archive when the account and group permissions allow it. Attachment
downloads obey the configured size and retention limits; a failed attachment
does not roll back an otherwise valid message page.

## Drafts and posting

Drafts are local and may contain bounded attachments. A draft can be queued,
reviewed and edited without contacting Groups.io. Final posting is a separate,
explicitly confirmed action.

Before that final provider call, RD-Nexus durably records a stable posting
attempt. A documented provider rejection returns the draft for correction. A
timeout, rate limit, server error, interrupted response or uncertain local
receipt becomes **delivery unknown** and is never retried automatically. Check
the recent Groups.io messages or server draft state first, then choose either
**Mark delivered** or **Allow one retry**. This operator reconciliation prevents
a restart or an ambiguous response from duplicating a post.

## Local data and disconnecting

**Clear downloaded cache** removes downloaded groups, topics and messages while
preserving local drafts, posting receipts and the API key. **Delete all local
data** also removes drafts and receipts. The API key remains in the credential
vault until **Disconnect** is used.

Live membership discovery, provider permissions, archive downloads, attachment
transfer and final posting require the operator's own Groups.io account and
network access. Local tests prove the client and recovery rules; they do not
stand in for an account-bound provider check.

See [Settings reference — Groups.io](settings-reference.md#groupsio) for every
control and default.
