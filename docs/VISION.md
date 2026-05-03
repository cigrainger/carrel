# Vision

## What we're building

A reading and curation tool for the open web — and for books, papers, podcasts, and everything else worth reading — that runs on your own computer, syncs peer-to-peer with the people you trust, and treats your attention as something to be honored rather than extracted.

It is, in spirit, a successor to Google Reader. But where Reader was a web product owned by an ad company that eventually killed it to chase a different ad product, this is a tool you run yourself, that you own, that no one can take away, and that's structured around a small group of people whose taste you trust rather than around a global feed and an algorithm.

The core acts are: subscribe to things, read them, notice passages, take notes, share what's worth sharing with people who want to see it, and build over time a personal record of what you've read and thought about. The architecture follows from those acts, not the other way around.

## Why this exists

When Google Reader was killed in 2013, what was lost wasn't really an RSS reader. RSS readers are abundant. What was lost was the social layer underneath: the small graph of trusted curators whose shared items you subscribed to, the quiet act of vouching for something with a brief note, the sense of an intellectual life shared with people you respected. That layer migrated to platforms — Twitter, Facebook, eventually Substack — where algorithms decide what you see, where engagement is the metric, where a take is rewarded over a recommendation, and where the people who run the infrastructure also have shareholders.

The open web didn't fail. It was outcompeted by products that captured the value of openness inside walls. RSS still works. Blogs still publish. The protocols are intact. What's missing is a way to be social *over* those protocols without going through a platform.

We also care about reading more broadly than RSS. The same intellectual life includes books, academic papers, podcasts, newsletters that arrive by email, articles found by accident on the open web. Existing tools are organized around the artifact (Goodreads for books, Zotero for papers, Feedly for feeds, Pocket for articles, Hypothesis for annotations) when the right organization is around the reader. You don't have a books life and a papers life and a blogs life. You have a reading life. The substrate happens to vary.

This project is the reader-shaped tool we wished existed: one place where what you read, what you noticed, what you saved, what you shared, and what your friends are reading all live together, on your own machine, under your own keys.

## Who this is for

Initially: us, and a small number of friends. People who miss Reader, who feel the absence of trusted curation, who want to track their reading without surveillance, who have something to say about what they read and someone they want to say it to.

Eventually: people who would have been Reader power users if Reader still existed. People who keep commonplace books, write marginalia, send articles to their friends, keep folders of "interesting things I read this year." People for whom reading is part of how they think, and who want a tool that respects that.

Not for: people who want a feed of trending takes. People who want to grow an audience. People who want to be told what to read. People who measure their reading life in streaks or counts. There are products for those needs, and they are the dominant products of our era. This is not one of them.

## Principles, in tension

These are the values, written as decisions we've already made when forced to choose:

**Tool, not platform.** A tool is something you reach for to do a job and then put down. A platform is something you live inside, that wants more of your time, that succeeds when you can't leave. Every design decision should push toward tool-ness: keyboard-driven, fast, focused, leaves you alone when you're not using it.

**Reading is the primary act.** Everything else — sharing, social, discovery, organization — is downstream. If a feature improves reading, it earns its place. If it competes with reading for the user's attention, it doesn't.

**Local-first, peer-to-peer, no servers we run.** Your data lives on your computer. Sync is between you and people you've added. There is no central service that can go down, get acquired, change its terms, or lose your stuff. We may run a static directory site to help people find each other, but it's not infrastructure anyone depends on.

**Trust is small and explicit.** You follow people you've added by public key. There is no global graph, no recommended-to-follow, no mutuals-of-mutuals algorithm. The cost of this is harder discovery; the benefit is that the social space stays small enough to be sane.

**No engagement metrics, anywhere.** No follower counts, no read counts, no like counts, no streaks, no notifications outside the app, no badges, no popularity sorting. These are the mechanisms by which a reading tool becomes an attention economy. We don't have them and we won't add them under any pressure.

**Honest read state.** The user marks things read when they've read them. The app does not pre-emptively clear unread counts to make the user feel productive, nor accumulate them to induce guilt. When in doubt, fewer signals over more.

**Sharing is vouching, not broadcasting.** When you share something, you're saying *this is worth your time* to a specific small group of people who chose to listen to you. It's the opposite of a tweet. It should feel deliberate and small, not performative.

**Speed is a feature.** Local operations are sub-frame. Reading interactions feel instant. Sync happens in the background and never blocks the reader. If a feature compromises the speed of the core reading loop, we don't ship it.

**Craft over ceremony.** The typography, the keyboard map, the keystroke-to-render latency, the feel of opening the next article — these are the things we polish. We do not polish onboarding flows, marketing pages, growth loops, or anything else that isn't the product itself.

**Resilience over freshness.** When you highlight a passage, we cache the article. When the publisher rewrites the page in three years, your highlight survives. When they delete it, your highlight survives. The tool's job is to make your reading life durable, not to be a thin pointer into someone else's database.

**Open formats throughout.** RSS in. EPUB out. OPML for portability. Standard cryptography. Public protocols where they exist. If our user wanted to leave for another tool, the data should walk with them. We treat lock-in as a failure mode, not a feature.

**Take ourselves seriously, take ourselves lightly.** This is a serious tool for a serious need. It's also a small project built by people who like building things. We can ship something craft-focused for ten people we like and have that be enough. We don't need to win.

## What we will not do

These are the anti-goals, listed explicitly so they're hard to drift into:

We will not add an algorithmic feed. The order of things is chronological or user-chosen, period.

We will not add recommendations ("you might also like"). The recommendation graph is the people you've chosen to follow. There is no other layer.

We will not add engagement metrics in any form. Follower counts, read counts, popularity, trending — none of these.

We will not build a public profile system. Your card on the directory site has your name, bio, and key. It does not have a stream of your activity for strangers to see.

We will not add notifications outside the app. The dock icon does not bounce. The system tray does not show counts. The app does not push.

We will not build advertising of any kind. Not affiliate links, not sponsored items, not "promoted content." The user is paying with attention; that's the whole budget.

We will not take VC money to grow this thing into something it isn't. If it costs money to run, it costs the people who use it (or the people who run their own peers). We are not building a venture-scale business.

We will not centralize. If we ever find ourselves designing infrastructure that has to run for the network to function, we have made a wrong turn and need to back up.

We will not add features because competitors have them. We will add features because users we respect would be better served by them.

We will not be acquired and turned into something else. If the project ends, it ends in source. The protocols and data are the user's, regardless of what happens to us.

## What "v1 done" looks like

A small group of people — initially us — using the app every day to read their RSS subscriptions, save articles from the open web, track books and papers they're reading, capture highlights and notes, and share things with each other in a way that feels like Reader did.

Concretely:

- Desktop app on macOS and Linux (Windows soon after) that's fast, keyboard-driven, and pleasant to read in
- Subscribes to RSS, Atom, and JSON feeds, fetches them politely, extracts readable content
- Imports OPML and Pocket exports
- Saves arbitrary URLs as articles with archived content
- Tracks books (with OpenLibrary metadata) and papers (with DOI/arXiv metadata)
- Highlights and notes on any item, durable across source changes
- Local data store with full-text search and the kind of cross-cutting queries the data model supports
- Self-sync between two of your own devices
- Following another user by public key, receiving their shares with notes
- Sharing with audience scoping (some friends, not all)
- Send-to-ereader for Kindle (email) and any device that speaks OPDS
- All of the above without notifications, counts, recommendations, or engagement loops

If we have those things, working well, used daily by ten people we know, we have shipped v1. Everything else is downstream.

## Beyond v1

We are deliberately not roadmapping past v1 in detail. The shape of phase 2 will be obvious from what's missing in phase 1 use. Likely directions, in rough order of plausibility:

- A mobile companion (read-only or light-write, syncing through a desktop peer)
- Threaded discussion on shares (not comments-on-everything, only on things deliberately opened for discussion)
- Better PDF reading and annotation
- Hypothesis interop for web annotations
- Import from Zotero, Kindle highlights, Apple Books, KOReader
- Shared lists between specific peers (book club, reading group)
- A small canonical directory site to help new users find each other

Anything not on this list is not ruled out, but should be argued for from first principles against the values above.

## On scope

Every project of this shape has a death by scope creep. Two patterns kill them:

The first is *building the general before the specific*. The data model and architecture we've chosen are general — they could in principle support a full social network, a wiki, a research platform. We resist building those generalizations until the specific reading tool is real and used. The substrate's generality is a *latent* property; we don't materialize it until forced to.

The second is *adding features to please hypothetical users*. We have real users (us, our friends) and we should serve them. A feature request from a hypothetical power-user we don't know is much weaker evidence than friction we feel ourselves. When in doubt, default to not adding the thing.

The corollary: when we *do* add things, they should fall out of the architecture rather than be layered on. If a feature requires bending the data model or adding a new subsystem, that's a signal to think harder. If it's two new fact types and a query, ship it.

## On working with AI

A non-trivial portion of this project will be built with AI assistance. This is a deliberate choice, not a compromise: the project is small, opinionated, and well-specified, which is exactly the kind of context where AI coding assistance shines.

But AI tools have a strong gravitational pull toward the median of their training data, which for software is engagement-product code: dashboards with metrics, growth loops, social features that maximize time-in-app, generic React components, and a thousand small concessions to "users want this." The project's principles are deliberately *against* the median. So:

When working with AI on this project, the principles in this document take precedence over anything an assistant might suggest is "best practice" or "what users expect." If an assistant proposes a feature that conflicts with the values here, push back. If it generates UI that competes for attention, redirect. If it suggests instrumentation, refuse. The taste of the project is the deliverable; the code is downstream of that taste.

Every significant change should be checkable against this document. If a change would require updating this document, that's a real decision to make consciously, not something to drift into.

## A closing note

This project exists because we miss something. The thing we miss isn't an RSS reader; it's a way of being on the internet that we lost. A small group of people who read carefully, shared what was worth sharing, and trusted each other's taste. A web that wasn't a feed. A reading life that was ours.

We don't think we can rebuild that for everyone. We think we can rebuild it for ourselves and for people who feel the same loss. That's a smaller, more honest goal than "fix the internet," and it's what we're building.
