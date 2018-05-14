use slack_hook::{Slack, PayloadBuilder, SlackLink, SlackText, SlackUserLink, AttachmentBuilder};
use slack_hook::SlackTextContent::{self, Text, Link, User};
use std::env;

use super::helm::helpers;
use super::structs::Metadata;
use super::{Result, ErrorKind};

/// Slack message options we support
///
/// These parameters get distilled into the attachments API.
/// Mostly because this is the only thing API that supports colour.
#[derive(Default, Debug)]
pub struct Message {
    /// Text in message
    pub text: String,

    /// Metadata from Manifest
    pub metadata: Option<Metadata>,

    /// Set when not wanting to niotify people
    pub quiet: bool,

    /// Optional color for the attachment API
    pub color: Option<String>,

    /// Optional code input
    pub code: Option<String>,

    /// Optional version to send when not having code diffs
    pub version: Option<String>,
}

pub fn env_hook_url() -> Result<String> {
    env::var("SLACK_SHIPCAT_HOOK_URL").map_err(|_| ErrorKind::MissingSlackUrl.into())
}
pub fn env_channel() -> Result<String> {
    env::var("SLACK_SHIPCAT_CHANNEL").map_err(|_| ErrorKind::MissingSlackChannel.into())
}
fn env_username() -> String {
    env::var("SLACK_SHIPCAT_NAME").unwrap_or_else(|_| "shipcat".into())
}

/// Basic check to see that slack credentials is working
///
/// Used before running upgrades so we have a trail
/// It's not very good at the moment. TODO: verify better
pub fn have_credentials() -> Result<()> {
    env_channel()?;
    env_hook_url()?;
    Ok(())
}

/// Send a `Message` to a configured slack destination
pub fn send(msg: Message) -> Result<()> {
    let hook_url : &str = &env_hook_url()?;
    let hook_chan : String = env_channel()?;
    let hook_user : String = env_username();
    // TODO: check hook url non-empty?

    let slack = Slack::new(hook_url).unwrap();
    let mut p = PayloadBuilder::new().channel(hook_chan)
      .icon_emoji(":ship:")
      .username(hook_user);

    debug!("Got slack notify {:?}", msg);
    // NB: cannot use .link_names due to https://api.slack.com/changelog/2017-09-the-one-about-usernames
    // NB: cannot use .parse(Parse::Full) as this breaks the other links
    // Thus we have to use full slack names, and construct SlackLink objs manually

    // All text is in either one or two attachments to make output as clean as possible

    // First attachment is main text + main link + CCs
    // Fallbacktext is in constructor here (shown in OSD notifies)
    let mut a = AttachmentBuilder::new(msg.text.clone()); // <- fallback
    if let Some(c) = msg.color {
        a = a.color(c)
    }
    // All text constructed for first attachment goes in this vec:
    let mut texts = vec![Text(msg.text.into())];

    if msg.code.is_some() && msg.metadata.is_none() {
        // TODO: only use this when notifying internally
        warn!("Not providing a slack github link due to missing metadata in manifest");
    }

    let mut have_gh_link = false;
    let mut codeattach = None;
    if let Some(code) = msg.code {
        if let Some(ref md) = msg.metadata {
            if let Some(lnk) = infer_metadata_links(md, &code) {
                have_gh_link = true;
                texts.push(lnk);
            }
        }
        let num_lines = code.lines().count();
        // if it's not a straight image change diff, print it:
        if !(num_lines == 3 && have_gh_link) {
            codeattach = Some(AttachmentBuilder::new(code.clone())
                .color("#439FE0")
                .text(vec![Text(code.into())].as_slice())
                .build()?);
        }
    } else if let Some(v) = msg.version {
        if let Some(ref md) = msg.metadata {
           texts.push(infer_metadata_single_link(md, v));
        }
    }

    // Auto link/text from originator
    texts.push(infer_ci_links());

    // Auto cc users
    if let Some(ref md) = msg.metadata {
        if !msg.quiet {
            texts.push(Text("cc ".to_string().into()));
            texts.extend(infer_slack_notifies(md));
        }
    }

    // Pass the texts array to slack_hook
    a = a.text(texts.as_slice());
    let mut ax = vec![a.build()?];

    // Second attachment: optional code (blue)
    if let Some(diffattach) = codeattach {
        ax.push(diffattach);
        // Pass attachment vector

    }
    p = p.attachments(ax);

    // Send everything. Phew.
    slack.send(&p.build()?)?;

    Ok(())
}

fn short_ver(ver: String) -> String {
    use semver::Version;
    if Version::parse(&ver).is_err() && ver.len() == 40 {
        // only abbreviate versions that are not semver and 40 chars (git shas)
        format!("{}", &ver[..8])
    } else {
        ver
    }
}

fn infer_metadata_single_link(md: &Metadata, ver: String) -> SlackTextContent {
    let url = format!("{}/commit/{}", md.repo, ver);
    Link(SlackLink::new(&url, &short_ver(ver)))
}

fn infer_metadata_links(md: &Metadata, diff: &str) -> Option<SlackTextContent> {
    if let Some((v1, v2)) = helpers::infer_version_change(&diff) {
        let url = format!("{}/compare/{}...{}", md.repo, v1, v2);
        Some(Link(SlackLink::new(&url, &short_ver(v2))))
    } else {
        None
    }
}

fn infer_slack_notifies(md: &Metadata) -> Vec<SlackTextContent> {
    md.contacts.iter().map(|cc| { User(SlackUserLink::new(&cc)) }).collect()
}

/// Infer originator of a message
fn infer_ci_links() -> SlackTextContent {
    if let (Ok(url), Ok(name), Ok(nr)) = (env::var("BUILD_URL"),
                                          env::var("JOB_NAME"),
                                          env::var("BUILD_NUMBER")) {
        // we are on jenkins
        Link(SlackLink::new(&url, &format!("{}#{}", name, nr)))
    } else if let (Ok(url), Ok(name), Ok(nr)) = (env::var("CIRCLE_BUILD_URL"),
                                                 env::var("CIRCLE_JOB"),
                                                 env::var("CIRCLE_BUILD_NUM")) {
        // we are on circle
        Link(SlackLink::new(&url, &format!("{}#{}", name, nr)))
    } else if let Ok(user) = env::var("USER") {
        Text(SlackText::new(format!("(via admin {})", user)))
    } else {
        warn!("Could not infer ci links from environment");
        Text(SlackText::new("via unknown user".to_string()))
    }
}


#[cfg(test)]
mod tests {
    use tests::setup;
    use super::super::{Manifest, Config};
    use super::{send, Message, env_channel};

    #[test]
    fn slack_test() {
        setup();
        let conf = Config::read().unwrap();
        let mf = Manifest::basic("fake-ask", &conf, Some("dev-uk".into())).unwrap();

        let chan = env_channel().unwrap();
        if chan == "#shipcat-test" {
          send(Message {
              text: format!("simple `{}` test", "slack"),
              ..Default::default()
          }).unwrap();
          send(Message {
                text: format!("Trivial upgrade deploy test of `{}`", "slack"),
                color: Some("good".into()),
                metadata: mf.metadata.clone(),
                code: Some(format!("Pod changed:
-  image: \"blah:e7c1e5dd5de74b2b5da5eef76eb5bf12bdc2ac19\"
+  image: \"blah:d4f01f5143643e75d9cc2d5e3221e82a9e1c12e5\"")),
                ..Default::default()
            }).unwrap();
          // this is not just a three line diff, so
          send(Message {
                text: format!("Non-trivial deploy test of `{}`", "slack"),
                color: Some("good".into()),
                metadata: mf.metadata,
                code: Some(format!("Pod changed:
-  value: \"somedeletedvar\"
-  image: \"blah:abc12345678\"
+  image: \"blah:abc23456789\"")),
                ..Default::default()
            }).unwrap();
        }
    }
}
