# justfile for Lexe's public monorepo.

set shell := ["bash", "-euo", "pipefail", "-c"]

mod app 'app'

# select a recipe interactively
default:
    @just --choose
