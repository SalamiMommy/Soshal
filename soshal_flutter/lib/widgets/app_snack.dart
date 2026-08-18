import 'package:flutter/material.dart';

void showAppSnack(BuildContext context, String message) {
  ScaffoldMessenger.of(context)
      .showSnackBar(SnackBar(content: SelectableText(message)));
}