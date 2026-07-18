The official Yaesu FT-991A CAT Operation Reference Manual PDF is not
committed to this repository (binary, ~1.4 MB). Download it before working
on `radio/`'s command table:

```
curl -L -o docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf \
  https://www.yaesu.com/Files/4CB893D7-1018-01AF-FA97E9E9AD48B50C/FT-991A_CAT_OM_ENG_1711-D.pdf
```

If `curl` without a browser-like `User-Agent` gets a connection reset from
`yaesu.com`, add one:

```
curl -L -A "Mozilla/5.0" -o docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf \
  https://www.yaesu.com/Files/4CB893D7-1018-01AF-FA97E9E9AD48B50C/FT-991A_CAT_OM_ENG_1711-D.pdf
```

CAT command reference: FT-991A CAT Operation Reference Manual (document
FT-991A_CAT_OM_ENG_1711-D), all page citations in `radio/`'s source and
`planning/` files refer to this manual's printed page footers.
