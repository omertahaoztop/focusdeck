# Screenshot demo

Made-up data for the README screenshots. `data/plugins/demo` is a fake plugin that answers in Turkish when `FOCUSDECK_LANG` starts with `tr` and in English otherwise; `home` is the English profile, `ev` the Turkish one.

```sh
cd docs/screenshots/demo
FOCUSDECK_CONFIG_DIR=$PWD/config FOCUSDECK_DATA_DIR=$PWD/data FOCUSDECK_STATE_DIR=$(mktemp -d) \
  LC_ALL=en_US.UTF-8 focusdeck --profile home
FOCUSDECK_CONFIG_DIR=$PWD/config FOCUSDECK_DATA_DIR=$PWD/data FOCUSDECK_STATE_DIR=$(mktemp -d) \
  LC_ALL=tr_TR.UTF-8 focusdeck --profile ev
```

The images are 2560x1440 renders of a 132x37 terminal: board, `Enter` on Errands (zoom), `m` (move), `?` (help).
