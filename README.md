**THIS CRATE IS A WORK-IN-PROGRESS OPEN SOURCING OF INTERNAL TOOLS.**

This crate has two main components:

- an [ACME](https://datatracker.ietf.org/doc/html/rfc8555) client that creates [cert-manager](https://cert-manager.io/) [certificates](https://cert-mahttps://datatracker.ietf.org/doc/html/rfc8555nager.io/docs/reference/api-docs/#cert-manager.io/v1.Certificate)
- a wrapper that serves an [axum::Router<()>](https://docs.rs/axum/latest/axum/struct.Router.html) over TLS using certificates provisioned by the above

I have been using both in production for over a year at [TODO: insert link to Transcribbit with a quick blurb, maybe mention recently having gotten permission to open source?]

The cert-manager CRD structs were created with [Kopium](https://crates.io/crates/kopium). In order to use this crate, you'll need to [install cert-manager in your cluster](https://cert-manager.io/docs/installation/)
TODO: discuss features, including aws-lc-rs and its alternatives

**ACME client**

TODO: add an example
TODO: describe necessary permissions

**Axum server wrapper**

TODO: example
