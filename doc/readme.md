# Gitview User Manual

[Gitview](https://github.com/accessware/gitview) is a native, keyboard-first GitHub client for Windows, made for screen reader users.

## System Requirements
- Windows 10 or 11, on x64 or Arm.
- A GitHub account.

## Installing
- `gitview_setup-x64.exe` installs Gitview. Use the `arm64` file on an Arm computer.
- `gitview-x64.zip` is a portable copy. Unzip it anywhere and run `gitview.exe`. It keeps its settings in its own folder.
- Gitview runs one copy at a time. Starting it again brings the running copy to the front.

## Signing In
- The first time Gitview starts, it asks you to sign in with GitHub.
- Gitview puts a sign-in code on the clipboard and opens GitHub in your browser. Paste the code there and approve Gitview. Gitview continues on its own when you do.
- Gitview keeps your sign-in in Windows Credential Manager, not in its settings file.
- To forget your sign-in, use `File -> Sign Out`.

## The Main Window
- The window has two lists: the views, then the items in the view you choose. `Tab` moves between them.
- `Enter` opens the selected item.
- The `Actions` menu has what you can do with the selected item. The context key and `Shift+F10` show the same menu on the item.
- Gitview loads more items when you reach the end of a list.
- Gitview refreshes the view on screen every minute. `F5` refreshes it now.

### Views
- Notifications: your GitHub notifications.
- Repositories: the repositories you own, work on, or reach through an organization.
- Starred: the repositories you starred.
- Watching: the repositories you watch.
- Assigned to me: the issues and pull requests assigned to you.
- Review requested: the pull requests waiting for your review.

To change the order of the views, select one and press `Shift+Up` or `Shift+Down`. To hide views, use the Views page of `File -> Settings...`.

### Filter and Sort
- The `Filter` menu chooses which items a view shows. Its choices change with the view:
  - Notifications: `Unread` or `All`.
  - Repositories: `All`, `Owned by me`, `Public`, or `Private`. `Hide forks` and `Hide archived` remove those from the list.
  - Assigned to me and Review requested: `Open`, `Closed`, or `All`.
- The `Sort` menu chooses the order of a view, when GitHub can give it in more than one order:
  - Repositories: `Last updated`, `Last pushed`, `Date created`, or `Name`.
  - Starred: `Date starred` or `Last updated`.
  - Assigned to me and Review requested: `Last updated`, `Date opened`, or `Most commented`.
- Gitview remembers the filter and the order of each view.

### Item Actions
- Open, which is `Enter`.
- Comment, on an issue or pull request.
- Close, as completed, as not planned, or as a duplicate of another issue. A pull request closes without merging.
- Reopen.
- Mark as read, Mark as done, and Unsubscribe, on a notification.
- Star or unstar, and watch or unwatch, on a repository.
- Copy, which copies the item as the list says it.
- Copy link.
- Open in browser.

## Repositories
`Enter` on a repository opens it in a dialog. The Sections list chooses what the list beside it shows:
- Commits, from the branch chosen in `Branch`. `Enter` on a commit shows its message and the files it changed.
- Issues and Pull requests that are open.
- Closed issues and Closed pull requests. Gitview loads these the first time you choose them.

`Enter` on an issue or pull request opens its thread.

To open any repository, use `File -> Go to Repository...`. Type `owner/name`, a link to anything in the repository, or only a name for one of your own.

## Users
`File -> Go to User...` opens a user by username, `@mention`, or link to their profile. The dialog has these sections:
- Profile: the name, bio, company, location, website, followers, following, public repositories, and join date. `Enter` on the website opens it.
- Repositories.
- Starred.
- Followers, with the most recent first.
- Following, with the most recent first. The organizations they follow come after the people.

An organization has only Profile, Repositories, and Followers. If an organization limits what third-party apps can see, a list says so. An owner of the organization can approve Gitview in its settings, under third-party access.

`Enter` on a repository opens it. `Enter` on a person opens them.

In the repository and user dialogs, `Ctrl+C`, `Ctrl+Shift+C`, and `Ctrl+B` copy the selected row, copy its link, and open it in your browser. They use the keys you set for Copy, Copy link, and Open in browser.

## Threads
- A thread lists the issue or pull request first, then its comments, oldest first.
- `Enter` on a post shows it formatted.
- The context menu of a post has `Reply with quote...`, `Copy text`, `Copy link`, and `Open in browser`.
- The `Comment...` button adds a comment. `Ctrl+Enter` posts it.
- In a comment, `@` suggests people to mention: the people in the thread first, then the people GitHub finds as you type. `Up` and `Down` choose one, `Enter` or `Tab` adds it, and `Escape` closes the list. `Ctrl+Space` shows the list again.

## Your Profile
`File -> Edit Profile...` changes your public GitHub profile: name, bio, website, company, location, and whether you're available for hire.

## Settings
`File -> Settings...` has these pages:
- Views: the views the main window shows.
- Display: `Show oldest entries first` reverses the lists. Threads, and the followers and following of a user, keep their order.
- Templates: what each kind of row says. See [Templates](#templates).
- Advanced: the update channel, whether to check for updates at startup, and how much Gitview writes to its log.

## Templates
A template sets what each kind of row says. A template is text with fields in double braces, such as `{{ title }}`, in [minijinja's syntax](https://docs.rs/minijinja/latest/minijinja/syntax/index.html). A part can depend on a field that is empty or false: `{% if description %}: {{ description }}{% endif %}`.

`Reset to default` restores a template. A template you didn't change follows the default, also when a later version of Gitview changes it. Gitview doesn't save a template with a mistake in it. If a template can't show a row, the row uses the default.

### Notifications
- `repository`: the repository it's in, such as `accessware/gitview`.
- `title`: what it's about, such as an issue's title.
- `kind`: what kind of thing it's about, such as `Issue`, `Pull request`, or `Release`.
- `reason`: why you got it, such as `mention` or `subscribed`.
- `unread`: whether you haven't read it.

Default: `{{ repository }}: {{ title }}. {{ kind }}, {{ reason }}`

### Repositories
- `name`: its owner and name, such as `accessware/gitview`.
- `description`: what it says it's for, or nothing.
- `private`: whether it's private.
- `fork`: whether it's a fork.
- `archived`: whether it's archived, and so read-only.
- `stars`: how many people starred it.
- `open_issues`: how many issues and pull requests are open.

Default: `{{ name }}{% if description %}: {{ description }}{% endif %}{% if private %}, private{% endif %}{% if fork %}, fork{% endif %}{% if archived %}, archived{% endif %}`

### Issues and Pull Requests
- `repository`: the repository it's in. It's empty in the repository's own dialog, where every row has the same one.
- `number`: its number, without the `#`.
- `title`: its title.
- `kind`: `issue` or `pull request`.
- `author`: who opened it.
- `comments`: how many comments it has, such as `1 comment` or `3 comments`.
- `state`: `open` or `closed`.
- `ending`: how a closed one ended: `completed`, `not planned`, `duplicate`, `merged`, or `closed`. It's empty while it's open.
- `time`: when it was opened.

Default: `{% if repository %}{{ repository }}: #{{ number }} {{ title }}, {{ kind }} by {% else %}#{{ number }} {{ title }}, by {% endif %}{{ author }}, {{ comments }}{% if ending %}, {{ ending }}{% endif %}`

### Commits
- `summary`: the first line of its message.
- `author`: the author's GitHub username, or their name from git when GitHub doesn't know them.
- `author_name`: the author's name from their git settings.
- `time`: when it was written.
- `sha`: its short ID, such as `de94f2a`.

Default: `{{ summary }}, by {{ author }}, {{ time }}, {{ sha }}`

### People
- `login`: their username.
- `name`: the name on their profile, when the list gives it.
- `organization`: whether it's an organization and not a person.

Default: `{{ login }}{% if organization %}, organization{% endif %}`

### Posts in a Thread
- `author`: who wrote it.
- `time`: when it was written.
- `text`: what it says.
- `opening`: whether it's the post that opened the thread, and not a comment.
- `kind`: `issue` or `pull request`, for the post that opened the thread. It's empty for comments.

Default, with the text on its own line: `{% if opening %}{{ author }} opened this {{ kind }} on {{ time }}{% else %}{{ author }} commented on {{ time }}{% endif %}:` then `{{ text }}`

## Keyboard Shortcuts
To change a key, use `File -> Customize Keyboard Shortcuts...`. The Main window tab has the keys for the menus. The Global tab has the keys that work in any program.

### Default Shortcuts
- `Enter`: Open.
- `Ctrl+R`: Comment.
- `Delete`: Mark as done.
- `Ctrl+C`: Copy.
- `Ctrl+Shift+C`: Copy link.
- `Ctrl+B`: Open in browser.
- `Ctrl+G`: Go to repository.
- `Ctrl+U`: Go to user.
- `Shift+Up` and `Shift+Down`: Move the selected view up or down.
- `F5`: Refresh.
- `Ctrl+,`: Settings.
- `Ctrl+Shift+E`: Edit profile.
- `Ctrl+Alt+G`, in any program: Show or hide Gitview.

These have no key until you give them one: the close actions, Reopen, Mark as read, Unsubscribe, Star or unstar, Watch or unwatch, Customize keyboard shortcuts, Sign out, and Check for updates.

## Hiding and the Tray
- The global shortcut, `Ctrl+Alt+G` by default, hides Gitview when you're in it and brings it back from anywhere. A dialog that is open hides and comes back with it.
- Gitview has an icon in the notification area. Click it to bring Gitview to the front. Its menu has `Show/Hide` and `Exit`.
- To quit Gitview, use `File -> Exit` or `Exit` in the tray menu. Closing the window also quits.

## Updates
- Gitview checks for updates when it starts. `Help -> Check for Updates...` checks now.
- The update channel is on the Advanced page of the settings. `Releases` offers new versions. `Development builds` offers the build of every change. A release of Gitview starts on Releases, and a development build on Development builds.
- Gitview checks the signature of an update before it installs it. Then it restarts.

## Settings File and Log
- Gitview keeps its settings in `gitview.toml`. The Advanced page of the settings shows the folder.
- Gitview writes `gitview.log` in the same folder. It can help with a problem report. The Advanced page sets how much it records, or turns it off.

## Changelog

### Version 0.1.0
- The first release of Gitview, a GitHub client for Windows.
