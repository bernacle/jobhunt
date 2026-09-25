# Test fixtures

`test_signing_key.pem` is an RSA key generated for these tests only
(`openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048`). It signs
the JWTs the tests present to the server; `test_signing_key.jwk.json` is its
public half as a JWK, served by the tests' mock identity provider. It
protects nothing.
