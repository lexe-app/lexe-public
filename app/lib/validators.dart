library;

import 'package:app_rs_dart/ffi/form.dart' as form;
import 'package:lexeapp/prelude.dart';

Result<String, String?> validatePassword(String? password) {
  if (password == null || password.isEmpty) {
    return const Err("");
  }

  // TODO(phlip9): this API should return a bare error enum and flutter should
  // convert that to a human-readable error message (for translations).
  final maybeErrMsg = form.validatePassword(password: password);
  if (maybeErrMsg == null) {
    return Ok(password);
  } else {
    return Err(maybeErrMsg);
  }
}

Result<String, String?> validateConfirmPassword({
  required String? password,
  required String? confirmPassword,
}) {
  if (confirmPassword == null || confirmPassword.isEmpty) {
    return const Err("");
  }

  if (password == confirmPassword) {
    return Ok(confirmPassword);
  } else if (password == null) {
    return const Err("");
  } else {
    return const Err("Passwords don't match");
  }
}
