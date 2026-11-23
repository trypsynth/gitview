# Gitview

An accessible GitHub client for the desktop, made for screen readers first.

## Templates

How each row reads is set by a template, which you can change in Settings, on the Templates page. A template is text with fields in double braces, such as `{{ title }}`, in [minijinja's syntax](https://docs.rs/minijinja/latest/minijinja/syntax/index.html). Parts can be left out when a field is empty or false: `{% if description %}: {{ description }}{% endif %}`.

Reset to default puts a template back the way it came. A template you've left alone follows the default, even if a later Gitview changes it. A template with a mistake in it isn't saved, and if one ever fails to show a row, the row uses the default instead.

### Notifications

- `repository`: the repository it's in, such as `trypsynth/gitview`.
- `title`: what it's about, such as an issue's title.
- `kind`: what it's about, such as `Issue`, `Pull request` or `Release`.
- `reason`: why you got it, such as `mention` or `subscribed`.
- `unread`: whether you haven't read it yet.

Default: `{{ repository }}: {{ title }}. {{ kind }}, {{ reason }}`

### Repositories

- `name`: its owner and name, such as `trypsynth/gitview`.
- `description`: what it says it's for, or nothing.
- `private`: whether it's private.
- `fork`: whether it's a fork.
- `archived`: whether it's archived, and so read-only.
- `stars`: how many have starred it.
- `open_issues`: how many issues and pull requests are open.

Default: `{{ name }}{% if description %}: {{ description }}{% endif %}{% if private %}, private{% endif %}{% if fork %}, fork{% endif %}{% if archived %}, archived{% endif %}`

### Issues and pull requests

- `repository`: the repository it's in. It's empty in a repository's own dialog, where every row shares it.
- `number`: its number, without the `#`.
- `title`: its title.
- `kind`: `issue` or `pull request`.
- `author`: who opened it.
- `comments`: how many comments it has, such as `1 comment` or `3 comments`.
- `state`: `open` or `closed`.
- `ending`: how a closed one ended, such as `completed`, `not planned`, `duplicate`, `merged` or `closed`. It's empty while it's open.
- `time`: when it was opened.

Default: `{% if repository %}{{ repository }}: #{{ number }} {{ title }}, {{ kind }} by {% else %}#{{ number }} {{ title }}, by {% endif %}{{ author }}, {{ comments }}{% if ending %}, {{ ending }}{% endif %}`

### Commits

- `summary`: the first line of its message.
- `author`: the author's GitHub username, or their name from git when GitHub doesn't know them.
- `author_name`: the author's name, as their git settings give it.
- `time`: when it was written.
- `sha`: its short id, such as `de94f2a`.

Default: `{{ summary }}, by {{ author }}, {{ time }}, {{ sha }}`

### People

- `login`: their username.
- `name`: the name on their profile, when the list gives it.
- `organization`: whether it's an organization rather than a person.

Default: `{{ login }}{% if organization %}, organization{% endif %}`

### Posts in a thread

- `author`: who wrote it.
- `time`: when it was written.
- `text`: what it says.
- `opening`: whether it opened the thread, rather than being a comment.
- `kind`: `issue` or `pull request`, for the post that opened the thread. It's empty for comments.

Default, with the text on a line of its own: `{% if opening %}{{ author }} opened this {{ kind }} on {{ time }}{% else %}{{ author }} commented on {{ time }}{% endif %}:` then `{{ text }}`
