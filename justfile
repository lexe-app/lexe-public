# justfile for Lexe's public monorepo.

set shell := ["bash", "-euo", "pipefail", "-c"]

mod app 'just/app/mod.just'

# select a recipe interactively
default:
    @just --choose
