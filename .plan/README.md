# Plan layout

This folder holds the Wayfinder map for the project. Commit it. It is the record of why decisions were made.

```
.plan/
  README.md              this file
  map.md                 the map, one per project
  TICKET.template.md     copy this to start a ticket
  tickets/               open tickets, one file each
  closed/                resolved tickets
```

## Rules

- Load `map.md` and the one ticket being worked. Never read the whole folder.
- Resolve one ticket per session. Research tickets are the exception.
- Name tickets `NNN-short-slug.md` with a three digit number. The next number is the highest number in `tickets/` and `closed/`, plus one.
- A ticket is on the frontier when its status is open, every ticket in `blocked_by` is closed, and `claimed` is false.

## Ticket types

- `research` gathers an outside fact. Mode is AFK, so a subagent can do it.
- `prototype` is a cheap, rough artifact to react to. Mode is HITL.
- `grilling` is a conversation with the user. This is the default. Mode is HITL.
- `task` is manual work needed before a decision, such as signing up for a service. Mode is HITL or AFK.

## Claiming and closing a ticket

1. Set `claimed: true` in the ticket.
2. Resolve it.
3. Fill in the Resolution section.
4. Set `status: closed` and `claimed: false`, then move the file from `tickets/` to `closed/`.
5. Add one line to Decisions so far in `map.md`, linking the closed file.
6. Update Frontier in `map.md`.
7. Create any tickets the decision surfaced. Move fog from Not yet specified into a ticket once the question can be stated sharply.
