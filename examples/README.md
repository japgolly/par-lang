# Examples

These programs show what it's like to build things in Par. To learn the
language itself, read the [book](https://par.run/book/).

## Big examples

Each big example is a complete web app in a package of its own. Its README
shows how to run it and walks through how it works.

- **[Chat](big/chat/):** a chat room in the browser. A central room serves
  every user at once.
- **[Downloader](big/downloader/):** downloads files into a `Downloads`
  directory. A web page starts the downloads, shows their progress live, and
  cancels them.
- **[Tasks](big/tasks/):** a task list in the browser, stored in SQLite. Every
  request is handled independently, so unlike the other two, there's no
  complicated concurrency.

## Small examples

The small examples share one package, `small/`, with one example per module.
Run one by passing its module to `par run`:

```sh
$ cd small
$ par run Sokoban
```

- **[HelloWorld](small/src/HelloWorld.par)** prints a greeting.
- **[Fibonacci](small/src/Fibonacci.par)** prints the first 30 Fibonacci
  numbers, taken from an infinite sequence built with an
  [iterative](https://par.run/book/types/iterative) type.
- **[CopyFile](small/src/CopyFile.par)** copies a file. It shows
  [error handling](https://par.run/book/quality_of_life/error_handling), and how
  open files close themselves when an error cuts the copying short.
- **[MiniGrep](small/src/MiniGrep.par)** searches the files in a directory for a
  piece of text, line by line. There's
  [a video](https://youtu.be/nU7Lt6k3lNQ) of coding it.
- **[Queue](small/src/Queue.par)** is a first-in, first-out queue where both
  pushing and popping take constant time. Its type combines an
  [iterative](https://par.run/book/types/iterative) and a
  [recursive](https://par.run/book/types/recursive) type.
- **[Deduplicate](small/src/Deduplicate.par)** removes repeated items from a
  list, using sets whose representation is hidden by an
  [existential type](https://par.run/book/types/exists).
- **[Sokoban](small/src/Sokoban.par)** is the box-pushing puzzle game, played in
  the terminal.
