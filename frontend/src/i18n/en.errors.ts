// The server's refusals a person sees: size, time, load and the values of settings. The server sends a stable `code` and an English sentence for scripts;
// the page says it in its own words (docs/API.md, "Errors"). The same keys as ru.errors.ts.
export default {
  'error.fieldTooLong': 'The field “{field}” is too long: at most {max} characters, now {actual}',
  'error.invalidName': 'The name must be 1 to {max} characters, now {actual}',
  'error.opsTooMany': 'One request takes at most {max} changes, and there are {actual}',
  'error.requestTimeout': 'The server did not answer in time and the request was stopped. Try again or simplify the design',
  'error.payloadTooLarge': 'The request is larger than the server accepts',
  'error.busy': 'The server is busy with other calculations. Try again in {seconds} s',
  'error.busy.soon': 'The server is busy with other calculations. Try again in a moment',
  'error.busy.user': 'You already have as many calculations running as the server runs for one person ({limit}). Wait for one to finish: about {seconds} s',
  'error.field.name': 'Name',
  'error.field.description': 'Description',
  'error.field.message': 'Message',
  'error.field.reason': 'Reason',
  'error.field.fingerprint': 'Finding fingerprint',
  'error.field.findingId': 'Finding id',

  // a setting value the server did not accept (`details.reasonCode` of the `invalid_setting` answer); below_min, above_max, not_an_option and wrong_type use the form's texts (admin.settings.err.*)
  'error.setting.entry': '“{value}”: {reason}',
  'error.setting.not_single_line': 'A single line is required: no line breaks or control characters',
  'error.setting.line_too_long': 'At most {max} characters',
  'error.setting.not_http': 'An address that starts with http:// or https:// is required',
  'error.setting.address_chars': 'The address must not contain spaces, backslashes or control characters',
  'error.setting.address_too_long': 'The address must be at most {max} characters',
  'error.setting.bad_address': 'This is not an address: check how it is written',
  'error.setting.no_host': 'The address names no host',
  'error.setting.has_credentials': 'The address must not contain a user name or a password',
  'error.setting.has_query': 'The address must not contain a query (?) or a fragment (#)',
  'error.setting.not_origin': 'An origin like scheme://host[:port] is required, without a path, a query or a fragment',
  'error.setting.empty': 'The value must not be empty',
  'error.setting.unknown_setting': 'There is no such setting',
  'error.setting.cannot_store': 'The database cannot store this value',
} as Record<string, string>
