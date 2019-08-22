#!/bin/bash

for c in $(find . -type f -name 'Cargo.toml'); do
	sed -i -e 's/^\(path\) =/#\1 =/;' $c
	if ! grep -q '^\[workspace\]' $c; then
		printf '\n[workspace]\n' >> $c
	fi
done
