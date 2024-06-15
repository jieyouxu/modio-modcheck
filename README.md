# mod.io modcheck

Check if there are hidden, renamed or deleted mods given a mods list exported by mint. This is
not needed on latest mint master branch, but this is a mitigation tool for people on mint stable.

```
Usage: modio-modcheck --id <USER_ID> --access-token <OAUTH2_ACCESS_TOKEN> <MOD_LIST>

Arguments:
  <MOD_LIST>

Options:
      --id <USER_ID>
      --access-token <OAUTH2_ACCESS_TOKEN>
  -h, --help                                Print help
```

- You can find User ID at [mod.io access][access].
- You are required to provide path to a file containing an OAuth2 token (also created in [mod.io
  access][access]).
- You are expected to provide path to a file containing a whitespace-delimited list of mods (this is
  the output of mint's Copy Profile URLs action).

[access]: https://mod.io/me/access
