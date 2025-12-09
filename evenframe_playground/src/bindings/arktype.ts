import { scope } from 'arktype';

export const bindings = scope({

Role: [[[['===', 'Admin'], '|', ['===', 'Moderator']], '|', ['===', 'User']], '|', ['===', 'Guest']],
PaymentMethod: [[[[[['===', 'CreditCard'], '|', ['===', 'DebitCard']], '|', ['===', 'BankTransfer']], '|', ['===', 'PayPal']], '|', ['===', 'Crypto']], '|', ['===', 'Cash']],
ProductCategory: [[[[[['===', 'Electronics'], '|', ['===', 'Clothing']], '|', ['===', 'Books']], '|', ['===', 'Home']], '|', ['===', 'Sports']], '|', ['===', 'Other']],
OrderStatus: [[[[['===', 'Pending'], '|', ['===', 'Processing']], '|', ['===', 'Shipped']], '|', ['===', 'Delivered']], '|', ['===', 'Cancelled']],
PaymentStatus: [[[[[['===', 'Pending'], '|', ['===', 'Processing']], '|', ['===', 'Completed']], '|', ['===', 'Failed']], '|', ['===', 'Refunded']], '|', ['===', 'Cancelled']],
MaxValidatorStacking: {
  id: 'string',
  megaValidatedString: 'string',
  optionalMegaValidated: [['string', '|', 'undefined'], '|', 'null'],
  megaValidatedNumber: 'number',
  megaValidatedU32: 'number',
  optionalMegaU32: [['number', '|', 'undefined'], '|', 'null'],
  megaValidatedI64: 'number',
  optionalMegaI64: [['number', '|', 'undefined'], '|', 'null']
},
Customer: {
  id: 'string',
  user: ['user', "|",  "string"],
  shippingAddress: [['address', '|', 'undefined'], '|', 'null'],
  billingAddress: [['address', '|', 'undefined'], '|', 'null'],
  phone: [['string', '|', 'undefined'], '|', 'null'],
  createdAt: 'string'
},
Order: {
  id: 'string',
  customer: ['customer', "|",  "string"],
  items: ['cartItem', '[]'],
  subtotal: 'number',
  tax: 'number',
  shippingCost: 'number',
  total: 'number',
  status: 'orderStatus',
  shippingAddress: 'address',
  notes: [['string', '|', 'undefined'], '|', 'null'],
  createdAt: 'string',
  shippedAt: [['string', '|', 'undefined'], '|', 'null'],
  deliveredAt: [['string', '|', 'undefined'], '|', 'null']
},
ArrayValidationExtremes: {
  id: 'string',
  tags: ['string', '[]'],
  validatedEmails: ['string', '[]'],
  optionalTags: [[['string', '[]'], '|', 'undefined'], '|', 'null'],
  scores: ['number', '[]'],
  optionalScores: [[['number', '[]'], '|', 'undefined'], '|', 'null'],
  measurements: ['number', '[]'],
  addresses: ['validatedAddress', '[]'],
  backupAddresses: [[['validatedAddress', '[]'], '|', 'undefined'], '|', 'null']
},
DateTimeExtremes: {
  id: 'string',
  createdAt: 'string',
  optionalUpdatedAt: [['string', '|', 'undefined'], '|', 'null'],
  birthDate: 'string',
  optionalExpiryDate: [['string', '|', 'undefined'], '|', 'null'],
  startTime: 'string',
  optionalEndTime: [['string', '|', 'undefined'], '|', 'null'],
  appointment: 'string',
  optionalFollowup: [['string', '|', 'undefined'], '|', 'null'],
  duration: 'string',
  optionalDuration: [['string', '|', 'undefined'], '|', 'null'],
  timezone: 'string',
  optionalTimezone: [['string', '|', 'undefined'], '|', 'null'],
  deadline: 'string',
  optionalTargetDate: [['string', '|', 'undefined'], '|', 'null']
},
NetworkTypes: {
  id: 'string',
  ipAddress: 'string',
  optionalIp: [['string', '|', 'undefined'], '|', 'null'],
  macAddress: 'string',
  optionalMac: [['string', '|', 'undefined'], '|', 'null'],
  internalApiUrl: 'string',
  optionalStorageUrl: [['string', '|', 'undefined'], '|', 'null'],
  userAgent: 'string',
  optionalUserAgent: [['string', '|', 'undefined'], '|', 'null']
},
NetworkTypes: {
  id: 'string',
  ipAddress: 'string',
  optionalIp: [['string', '|', 'undefined'], '|', 'null'],
  macAddress: 'string',
  optionalMac: [['string', '|', 'undefined'], '|', 'null'],
  internalApiUrl: 'string',
  optionalStorageUrl: [['string', '|', 'undefined'], '|', 'null'],
  userAgent: 'string',
  optionalUserAgent: [['string', '|', 'undefined'], '|', 'null']
},
Session: {
  id: 'string',
  user: ['user', "|",  "string"],
  token: 'string',
  expiresAt: 'string',
  createdAt: 'string'
},
ComplexStringValidation: {
  id: 'string',
  optionalUuid: [['string', '|', 'undefined'], '|', 'null'],
  optionalIp: [['string', '|', 'undefined'], '|', 'null'],
  optionalIpv4: [['string', '|', 'undefined'], '|', 'null'],
  optionalDigits: [['string', '|', 'undefined'], '|', 'null'],
  optionalHex: [['string', '|', 'undefined'], '|', 'null'],
  optionalAlpha: [['string', '|', 'undefined'], '|', 'null'],
  optionalAlphanumeric: [['string', '|', 'undefined'], '|', 'null'],
  optionalDomain: [['string', '|', 'undefined'], '|', 'null'],
  optionalWithAt: [['string', '|', 'undefined'], '|', 'null']
},
FloatValidation: {
  id: 'string',
  positiveFloat: 'number',
  nonNegativeFloat: 'number',
  normalized: 'number',
  optionalPositiveFloat: [['number', '|', 'undefined'], '|', 'null']
},
NonEmptyOptionString: {
  id: 'string',
  optionalNonEmpty: [['string', '|', 'undefined'], '|', 'null'],
  optionalMinThree: [['string', '|', 'undefined'], '|', 'null']
},
ComplexIdentifiers: {
  id: 'string',
  uuidField: 'string',
  optionalUuid: [['string', '|', 'undefined'], '|', 'null'],
  hexId32: 'string',
  optionalHexId: [['string', '|', 'undefined'], '|', 'null'],
  base64Token: 'string',
  optionalBase64Token: [['string', '|', 'undefined'], '|', 'null'],
  version: 'string',
  optionalVersion: [['string', '|', 'undefined'], '|', 'null'],
  sha256Hash: 'string',
  optionalHash: [['string', '|', 'undefined'], '|', 'null']
},
Post: {
  id: 'string',
  title: 'string',
  slug: 'string',
  content: 'string',
  excerpt: [['string', '|', 'undefined'], '|', 'null'],
  author: ['author', "|",  "string"],
  tags: [['tag', "|",  "string"], '[]'],
  featuredImage: [['string', '|', 'undefined'], '|', 'null'],
  published: 'boolean',
  publishedAt: [['string', '|', 'undefined'], '|', 'null'],
  viewCount: 'number',
  createdAt: 'string',
  updatedAt: 'string'
},
KitchenSinkString: {
  id: 'string',
  strictUsername: 'string',
  optionalStrictUsername: [['string', '|', 'undefined'], '|', 'null'],
  validatedEmail: 'string',
  optionalValidatedEmail: [['string', '|', 'undefined'], '|', 'null'],
  apiEndpoint: 'string',
  optionalCdnUrl: [['string', '|', 'undefined'], '|', 'null']
},
ComplexIdentifiers: {
  id: 'string',
  uuidField: 'string',
  optionalUuid: [['string', '|', 'undefined'], '|', 'null'],
  hexId32: 'string',
  optionalHexId: [['string', '|', 'undefined'], '|', 'null'],
  base64Token: 'string',
  optionalBase64Token: [['string', '|', 'undefined'], '|', 'null'],
  version: 'string',
  optionalVersion: [['string', '|', 'undefined'], '|', 'null'],
  sha256Hash: 'string',
  optionalHash: [['string', '|', 'undefined'], '|', 'null']
},
MultipleNumberValidators: {
  id: 'string',
  boundedPositive: 'number',
  optionalByte: [['number', '|', 'undefined'], '|', 'null'],
  percentage: 'number'
},
BusinessExtremes: {
  id: 'string',
  companyName: 'string',
  jobTitle: 'string',
  optionalParentCompany: [['string', '|', 'undefined'], '|', 'null'],
  productName: 'string',
  sku: 'string',
  optionalVariantName: [['string', '|', 'undefined'], '|', 'null'],
  optionalVariantSku: [['string', '|', 'undefined'], '|', 'null'],
  price: 'number',
  optionalDiscount: [['number', '|', 'undefined'], '|', 'null'],
  quantity: 'number',
  optionalMinQuantity: [['number', '|', 'undefined'], '|', 'null'],
  testCardNumber: 'string',
  optionalBackupCard: [['string', '|', 'undefined'], '|', 'null']
},
DeepNestedValidation: {
  id: 'string',
  name: 'string',
  primaryAddress: 'validatedAddress',
  billingAddress: [['validatedAddress', '|', 'undefined'], '|', 'null'],
  shippingAddresses: ['validatedAddress', '[]'],
  primaryContact: 'validatedContact',
  secondaryContact: [['validatedContact', '|', 'undefined'], '|', 'null'],
  additionalContacts: ['validatedContact', '[]'],
  totalOrders: 'number',
  totalSpent: 'number',
  creditLimit: [['number', '|', 'undefined'], '|', 'null']
},
EdgeCasePost: {
  id: 'string',
  author: ['edgeCaseUser', "|",  "string"],
  title: 'string',
  content: 'string',
  slug: 'string',
  createdAt: 'string',
  updatedAt: [['string', '|', 'undefined'], '|', 'null'],
  viewCount: 'number',
  likeCount: 'number',
  isPublished: 'boolean',
  featuredImage: [['string', '|', 'undefined'], '|', 'null'],
  metaDescription: [['string', '|', 'undefined'], '|', 'null']
},
OptionalIntegerValidation: {
  id: 'string',
  optionalPositiveU32: [['number', '|', 'undefined'], '|', 'null'],
  optionalNonNegativeI32: [['number', '|', 'undefined'], '|', 'null'],
  optionalPositiveU64: [['number', '|', 'undefined'], '|', 'null'],
  optionalNegativeI64: [['number', '|', 'undefined'], '|', 'null']
},
EdgeCasePost: {
  id: 'string',
  author: ['edgeCaseUser', "|",  "string"],
  title: 'string',
  content: 'string',
  slug: 'string',
  createdAt: 'string',
  updatedAt: [['string', '|', 'undefined'], '|', 'null'],
  viewCount: 'number',
  likeCount: 'number',
  isPublished: 'boolean',
  featuredImage: [['string', '|', 'undefined'], '|', 'null'],
  metaDescription: [['string', '|', 'undefined'], '|', 'null']
},
Product: {
  id: 'string',
  name: 'string',
  description: 'string',
  price: 'number',
  stockQuantity: 'number',
  category: 'productCategory',
  imageUrl: [['string', '|', 'undefined'], '|', 'null'],
  isAvailable: 'boolean',
  createdAt: 'string'
},
ExtremeIntegerValidation: {
  id: 'string',
  constrainedU8: 'number',
  optionalConstrainedU8: [['number', '|', 'undefined'], '|', 'null'],
  boundedU16: 'number',
  optionalBoundedU16: [['number', '|', 'undefined'], '|', 'null'],
  percentageU32: 'number',
  optionalPercentageU32: [['number', '|', 'undefined'], '|', 'null'],
  largeU64: 'number',
  optionalLargeU64: [['number', '|', 'undefined'], '|', 'null'],
  negativeI8: 'number',
  optionalNegativeI8: [['number', '|', 'undefined'], '|', 'null'],
  nonPositiveI16: 'number',
  optionalNonPositiveI16: [['number', '|', 'undefined'], '|', 'null'],
  boundedI32: 'number',
  optionalBoundedI32: [['number', '|', 'undefined'], '|', 'null'],
  constrainedI64: 'number',
  optionalNonNegativeI64: [['number', '|', 'undefined'], '|', 'null'],
  boundedUsize: 'number',
  optionalBoundedUsize: [['number', '|', 'undefined'], '|', 'null'],
  boundedIsize: 'number',
  optionalPositiveIsize: [['number', '|', 'undefined'], '|', 'null']
},
User: {
  id: 'string',
  email: 'string',
  username: 'string',
  passwordHash: 'string',
  roles: ['role', '[]'],
  isActive: 'boolean',
  createdAt: 'string',
  updatedAt: 'string'
},
Tag: {
  id: 'string',
  name: 'string',
  slug: 'string',
  description: [['string', '|', 'undefined'], '|', 'null'],
  postCount: 'number'
},
EdgeCaseUser: {
  id: 'string',
  email: 'string',
  username: 'string',
  createdAt: 'string',
  loginCount: 'number',
  rating: [['number', '|', 'undefined'], '|', 'null']
},
ValidatedAddress: {
  street: 'string',
  city: 'string',
  stateCode: 'string',
  postalCode: 'string',
  countryCode: 'string',
  latitude: [['number', '|', 'undefined'], '|', 'null'],
  longitude: [['number', '|', 'undefined'], '|', 'null']
},
PersonalInfoExtremes: {
  id: 'string',
  firstName: 'string',
  lastName: 'string',
  fullName: 'string',
  optionalNickname: [['string', '|', 'undefined'], '|', 'null'],
  phone: 'string',
  optionalMobile: [['string', '|', 'undefined'], '|', 'null'],
  email: 'string',
  optionalWorkEmail: [['string', '|', 'undefined'], '|', 'null'],
  street: 'string',
  city: 'string',
  state: 'string',
  postalCode: 'string',
  country: 'string',
  optionalStreet: [['string', '|', 'undefined'], '|', 'null'],
  optionalCity: [['string', '|', 'undefined'], '|', 'null']
},
DeepNestedValidation: {
  id: 'string',
  name: 'string',
  primaryAddress: 'validatedAddress',
  billingAddress: [['validatedAddress', '|', 'undefined'], '|', 'null'],
  shippingAddresses: ['validatedAddress', '[]'],
  primaryContact: 'validatedContact',
  secondaryContact: [['validatedContact', '|', 'undefined'], '|', 'null'],
  additionalContacts: ['validatedContact', '[]'],
  totalOrders: 'number',
  totalSpent: 'number',
  creditLimit: [['number', '|', 'undefined'], '|', 'null']
},
OptionalIntegerValidation: {
  id: 'string',
  optionalPositiveU32: [['number', '|', 'undefined'], '|', 'null'],
  optionalNonNegativeI32: [['number', '|', 'undefined'], '|', 'null'],
  optionalPositiveU64: [['number', '|', 'undefined'], '|', 'null'],
  optionalNegativeI64: [['number', '|', 'undefined'], '|', 'null']
},
Comment: {
  id: 'string',
  post: ['post', "|",  "string"],
  author: ['author', "|",  "string"],
  content: 'string',
  parentCommentId: [['string', '|', 'undefined'], '|', 'null'],
  isApproved: 'boolean',
  createdAt: 'string',
  editedAt: [['string', '|', 'undefined'], '|', 'null']
},
ComplexPayment: {
  id: 'string',
  user: ['edgeCaseUser', "|",  "string"],
  status: 'paymentStatus',
  method: 'paymentMethod',
  amount: 'number',
  currency: 'string',
  fee: 'number',
  tax: 'number',
  total: 'number',
  transactionId: 'string',
  referenceCode: [['string', '|', 'undefined'], '|', 'null'],
  createdAt: 'string',
  processedAt: [['string', '|', 'undefined'], '|', 'null'],
  completedAt: [['string', '|', 'undefined'], '|', 'null'],
  billingAddress: 'validatedAddress',
  notes: [['string', '|', 'undefined'], '|', 'null'],
  retryCount: 'number',
  failureReason: [['string', '|', 'undefined'], '|', 'null']
},
FloatValidation: {
  id: 'string',
  positiveFloat: 'number',
  nonNegativeFloat: 'number',
  normalized: 'number',
  optionalPositiveFloat: [['number', '|', 'undefined'], '|', 'null']
},
Session: {
  id: 'string',
  user: ['user', "|",  "string"],
  token: 'string',
  expiresAt: 'string',
  createdAt: 'string'
},
ExtremeIntegerValidation: {
  id: 'string',
  constrainedU8: 'number',
  optionalConstrainedU8: [['number', '|', 'undefined'], '|', 'null'],
  boundedU16: 'number',
  optionalBoundedU16: [['number', '|', 'undefined'], '|', 'null'],
  percentageU32: 'number',
  optionalPercentageU32: [['number', '|', 'undefined'], '|', 'null'],
  largeU64: 'number',
  optionalLargeU64: [['number', '|', 'undefined'], '|', 'null'],
  negativeI8: 'number',
  optionalNegativeI8: [['number', '|', 'undefined'], '|', 'null'],
  nonPositiveI16: 'number',
  optionalNonPositiveI16: [['number', '|', 'undefined'], '|', 'null'],
  boundedI32: 'number',
  optionalBoundedI32: [['number', '|', 'undefined'], '|', 'null'],
  constrainedI64: 'number',
  optionalNonNegativeI64: [['number', '|', 'undefined'], '|', 'null'],
  boundedUsize: 'number',
  optionalBoundedUsize: [['number', '|', 'undefined'], '|', 'null'],
  boundedIsize: 'number',
  optionalPositiveIsize: [['number', '|', 'undefined'], '|', 'null']
},
CaseValidation: {
  id: 'string',
  optionalLowercase: [['string', '|', 'undefined'], '|', 'null'],
  optionalUppercase: [['string', '|', 'undefined'], '|', 'null'],
  optionalCapitalized: [['string', '|', 'undefined'], '|', 'null'],
  optionalUncapitalized: [['string', '|', 'undefined'], '|', 'null'],
  optionalTrimmed: [['string', '|', 'undefined'], '|', 'null']
},
IntegerBounds: {
  id: 'string',
  percentage: 'number',
  aboveZero: 'number',
  underThousand: 'number',
  optionalByteValue: [['number', '|', 'undefined'], '|', 'null'],
  optionalBounded: [['number', '|', 'undefined'], '|', 'null']
},
MultiValidatorOptionalString: {
  id: 'string',
  username: [['string', '|', 'undefined'], '|', 'null'],
  email: [['string', '|', 'undefined'], '|', 'null'],
  website: [['string', '|', 'undefined'], '|', 'null']
},
InnerValidated: {
  name: 'string',
  count: 'number',
  optionalEmail: [['string', '|', 'undefined'], '|', 'null']
},
Address: {
  street: 'string',
  city: 'string',
  state: 'string',
  postalCode: 'string',
  country: 'string'
},
EdgeCaseComment: {
  id: 'string',
  post: ['edgeCasePost', "|",  "string"],
  author: ['edgeCaseUser', "|",  "string"],
  content: 'string',
  createdAt: 'string',
  editedAt: [['string', '|', 'undefined'], '|', 'null'],
  likeCount: 'number',
  parentCommentId: [['string', '|', 'undefined'], '|', 'null'],
  isApproved: 'boolean',
  authorIp: [['string', '|', 'undefined'], '|', 'null']
},
ExtremeFloatValidation: {
  id: 'string',
  multiConstrainedF32: 'number',
  optionalF32: [['number', '|', 'undefined'], '|', 'null'],
  normalizedF64: 'number',
  optionalNormalizedF64: [['number', '|', 'undefined'], '|', 'null'],
  negativeF64: 'number',
  optionalNegativeF64: [['number', '|', 'undefined'], '|', 'null'],
  currencyAmount: 'number',
  optionalCurrency: [['number', '|', 'undefined'], '|', 'null'],
  percentage: 'number',
  optionalPercentage: [['number', '|', 'undefined'], '|', 'null'],
  latitude: 'number',
  longitude: 'number',
  optionalLatitude: [['number', '|', 'undefined'], '|', 'null'],
  optionalLongitude: [['number', '|', 'undefined'], '|', 'null']
},
BusinessExtremes: {
  id: 'string',
  companyName: 'string',
  jobTitle: 'string',
  optionalParentCompany: [['string', '|', 'undefined'], '|', 'null'],
  productName: 'string',
  sku: 'string',
  optionalVariantName: [['string', '|', 'undefined'], '|', 'null'],
  optionalVariantSku: [['string', '|', 'undefined'], '|', 'null'],
  price: 'number',
  optionalDiscount: [['number', '|', 'undefined'], '|', 'null'],
  quantity: 'number',
  optionalMinQuantity: [['number', '|', 'undefined'], '|', 'null'],
  testCardNumber: 'string',
  optionalBackupCard: [['string', '|', 'undefined'], '|', 'null']
},
Post: {
  id: 'string',
  title: 'string',
  slug: 'string',
  content: 'string',
  excerpt: [['string', '|', 'undefined'], '|', 'null'],
  author: ['author', "|",  "string"],
  tags: [['tag', "|",  "string"], '[]'],
  featuredImage: [['string', '|', 'undefined'], '|', 'null'],
  published: 'boolean',
  publishedAt: [['string', '|', 'undefined'], '|', 'null'],
  viewCount: 'number',
  createdAt: 'string',
  updatedAt: 'string'
},
Customer: {
  id: 'string',
  user: ['user', "|",  "string"],
  shippingAddress: [['address', '|', 'undefined'], '|', 'null'],
  billingAddress: [['address', '|', 'undefined'], '|', 'null'],
  phone: [['string', '|', 'undefined'], '|', 'null'],
  createdAt: 'string'
},
MaxValidatorStacking: {
  id: 'string',
  megaValidatedString: 'string',
  optionalMegaValidated: [['string', '|', 'undefined'], '|', 'null'],
  megaValidatedNumber: 'number',
  megaValidatedU32: 'number',
  optionalMegaU32: [['number', '|', 'undefined'], '|', 'null'],
  megaValidatedI64: 'number',
  optionalMegaI64: [['number', '|', 'undefined'], '|', 'null']
},
User: {
  id: 'string',
  email: 'string',
  username: 'string',
  passwordHash: 'string',
  roles: ['role', '[]'],
  isActive: 'boolean',
  createdAt: 'string',
  updatedAt: 'string'
},
Product: {
  id: 'string',
  name: 'string',
  description: 'string',
  price: 'number',
  stockQuantity: 'number',
  category: 'productCategory',
  imageUrl: [['string', '|', 'undefined'], '|', 'null'],
  isAvailable: 'boolean',
  createdAt: 'string'
},
Author: {
  id: 'string',
  user: ['user', "|",  "string"],
  bio: [['string', '|', 'undefined'], '|', 'null'],
  avatarUrl: [['string', '|', 'undefined'], '|', 'null'],
  twitterHandle: [['string', '|', 'undefined'], '|', 'null'],
  githubHandle: [['string', '|', 'undefined'], '|', 'null'],
  createdAt: 'string'
},
Order: {
  id: 'string',
  customer: ['customer', "|",  "string"],
  items: ['cartItem', '[]'],
  subtotal: 'number',
  tax: 'number',
  shippingCost: 'number',
  total: 'number',
  status: 'orderStatus',
  shippingAddress: 'address',
  notes: [['string', '|', 'undefined'], '|', 'null'],
  createdAt: 'string',
  shippedAt: [['string', '|', 'undefined'], '|', 'null'],
  deliveredAt: [['string', '|', 'undefined'], '|', 'null']
},
Tag: {
  id: 'string',
  name: 'string',
  slug: 'string',
  description: [['string', '|', 'undefined'], '|', 'null'],
  postCount: 'number'
},
Comment: {
  id: 'string',
  post: ['post', "|",  "string"],
  author: ['author', "|",  "string"],
  content: 'string',
  parentCommentId: [['string', '|', 'undefined'], '|', 'null'],
  isApproved: 'boolean',
  createdAt: 'string',
  editedAt: [['string', '|', 'undefined'], '|', 'null']
},
CaseValidation: {
  id: 'string',
  optionalLowercase: [['string', '|', 'undefined'], '|', 'null'],
  optionalUppercase: [['string', '|', 'undefined'], '|', 'null'],
  optionalCapitalized: [['string', '|', 'undefined'], '|', 'null'],
  optionalUncapitalized: [['string', '|', 'undefined'], '|', 'null'],
  optionalTrimmed: [['string', '|', 'undefined'], '|', 'null']
},
ValidatedContact: {
  name: 'string',
  email: 'string',
  phone: [['string', '|', 'undefined'], '|', 'null'],
  title: [['string', '|', 'undefined'], '|', 'null']
},
NonEmptyOptionString: {
  id: 'string',
  optionalNonEmpty: [['string', '|', 'undefined'], '|', 'null'],
  optionalMinThree: [['string', '|', 'undefined'], '|', 'null']
},
MultiValidatorOptionalString: {
  id: 'string',
  username: [['string', '|', 'undefined'], '|', 'null'],
  email: [['string', '|', 'undefined'], '|', 'null'],
  website: [['string', '|', 'undefined'], '|', 'null']
},
Author: {
  id: 'string',
  user: ['user', "|",  "string"],
  bio: [['string', '|', 'undefined'], '|', 'null'],
  avatarUrl: [['string', '|', 'undefined'], '|', 'null'],
  twitterHandle: [['string', '|', 'undefined'], '|', 'null'],
  githubHandle: [['string', '|', 'undefined'], '|', 'null'],
  createdAt: 'string'
},
ComplexPayment: {
  id: 'string',
  user: ['edgeCaseUser', "|",  "string"],
  status: 'paymentStatus',
  method: 'paymentMethod',
  amount: 'number',
  currency: 'string',
  fee: 'number',
  tax: 'number',
  total: 'number',
  transactionId: 'string',
  referenceCode: [['string', '|', 'undefined'], '|', 'null'],
  createdAt: 'string',
  processedAt: [['string', '|', 'undefined'], '|', 'null'],
  completedAt: [['string', '|', 'undefined'], '|', 'null'],
  billingAddress: 'validatedAddress',
  notes: [['string', '|', 'undefined'], '|', 'null'],
  retryCount: 'number',
  failureReason: [['string', '|', 'undefined'], '|', 'null']
},
AllIntegerTypes: {
  id: 'string',
  positiveU8: 'number',
  positiveU16: 'number',
  positiveU32: 'number',
  positiveU64: 'number',
  nonNegativeUsize: 'number',
  positiveI8: 'number',
  positiveI16: 'number',
  positiveI32: 'number',
  positiveI64: 'number',
  nonNegativeIsize: 'number'
},
DateTimeExtremes: {
  id: 'string',
  createdAt: 'string',
  optionalUpdatedAt: [['string', '|', 'undefined'], '|', 'null'],
  birthDate: 'string',
  optionalExpiryDate: [['string', '|', 'undefined'], '|', 'null'],
  startTime: 'string',
  optionalEndTime: [['string', '|', 'undefined'], '|', 'null'],
  appointment: 'string',
  optionalFollowup: [['string', '|', 'undefined'], '|', 'null'],
  duration: 'string',
  optionalDuration: [['string', '|', 'undefined'], '|', 'null'],
  timezone: 'string',
  optionalTimezone: [['string', '|', 'undefined'], '|', 'null'],
  deadline: 'string',
  optionalTargetDate: [['string', '|', 'undefined'], '|', 'null']
},
PersonalInfoExtremes: {
  id: 'string',
  firstName: 'string',
  lastName: 'string',
  fullName: 'string',
  optionalNickname: [['string', '|', 'undefined'], '|', 'null'],
  phone: 'string',
  optionalMobile: [['string', '|', 'undefined'], '|', 'null'],
  email: 'string',
  optionalWorkEmail: [['string', '|', 'undefined'], '|', 'null'],
  street: 'string',
  city: 'string',
  state: 'string',
  postalCode: 'string',
  country: 'string',
  optionalStreet: [['string', '|', 'undefined'], '|', 'null'],
  optionalCity: [['string', '|', 'undefined'], '|', 'null']
},
ExtremeFloatValidation: {
  id: 'string',
  multiConstrainedF32: 'number',
  optionalF32: [['number', '|', 'undefined'], '|', 'null'],
  normalizedF64: 'number',
  optionalNormalizedF64: [['number', '|', 'undefined'], '|', 'null'],
  negativeF64: 'number',
  optionalNegativeF64: [['number', '|', 'undefined'], '|', 'null'],
  currencyAmount: 'number',
  optionalCurrency: [['number', '|', 'undefined'], '|', 'null'],
  percentage: 'number',
  optionalPercentage: [['number', '|', 'undefined'], '|', 'null'],
  latitude: 'number',
  longitude: 'number',
  optionalLatitude: [['number', '|', 'undefined'], '|', 'null'],
  optionalLongitude: [['number', '|', 'undefined'], '|', 'null']
},
ComplexStringValidation: {
  id: 'string',
  optionalUuid: [['string', '|', 'undefined'], '|', 'null'],
  optionalIp: [['string', '|', 'undefined'], '|', 'null'],
  optionalIpv4: [['string', '|', 'undefined'], '|', 'null'],
  optionalDigits: [['string', '|', 'undefined'], '|', 'null'],
  optionalHex: [['string', '|', 'undefined'], '|', 'null'],
  optionalAlpha: [['string', '|', 'undefined'], '|', 'null'],
  optionalAlphanumeric: [['string', '|', 'undefined'], '|', 'null'],
  optionalDomain: [['string', '|', 'undefined'], '|', 'null'],
  optionalWithAt: [['string', '|', 'undefined'], '|', 'null']
},
OuterWithNestedValidator: {
  id: 'string',
  inner: 'innerValidated',
  optionalInner: [['innerValidated', '|', 'undefined'], '|', 'null'],
  innerList: ['innerValidated', '[]']
},
EdgeCaseUser: {
  id: 'string',
  email: 'string',
  username: 'string',
  createdAt: 'string',
  loginCount: 'number',
  rating: [['number', '|', 'undefined'], '|', 'null']
},
KitchenSinkString: {
  id: 'string',
  strictUsername: 'string',
  optionalStrictUsername: [['string', '|', 'undefined'], '|', 'null'],
  validatedEmail: 'string',
  optionalValidatedEmail: [['string', '|', 'undefined'], '|', 'null'],
  apiEndpoint: 'string',
  optionalCdnUrl: [['string', '|', 'undefined'], '|', 'null']
},
ArrayValidationExtremes: {
  id: 'string',
  tags: ['string', '[]'],
  validatedEmails: ['string', '[]'],
  optionalTags: [[['string', '[]'], '|', 'undefined'], '|', 'null'],
  scores: ['number', '[]'],
  optionalScores: [[['number', '[]'], '|', 'undefined'], '|', 'null'],
  measurements: ['number', '[]'],
  addresses: ['validatedAddress', '[]'],
  backupAddresses: [[['validatedAddress', '[]'], '|', 'undefined'], '|', 'null']
},
IntegerBounds: {
  id: 'string',
  percentage: 'number',
  aboveZero: 'number',
  underThousand: 'number',
  optionalByteValue: [['number', '|', 'undefined'], '|', 'null'],
  optionalBounded: [['number', '|', 'undefined'], '|', 'null']
},
OuterWithNestedValidator: {
  id: 'string',
  inner: 'innerValidated',
  optionalInner: [['innerValidated', '|', 'undefined'], '|', 'null'],
  innerList: ['innerValidated', '[]']
},
EdgeCaseComment: {
  id: 'string',
  post: ['edgeCasePost', "|",  "string"],
  author: ['edgeCaseUser', "|",  "string"],
  content: 'string',
  createdAt: 'string',
  editedAt: [['string', '|', 'undefined'], '|', 'null'],
  likeCount: 'number',
  parentCommentId: [['string', '|', 'undefined'], '|', 'null'],
  isApproved: 'boolean',
  authorIp: [['string', '|', 'undefined'], '|', 'null']
},
MultipleNumberValidators: {
  id: 'string',
  boundedPositive: 'number',
  optionalByte: [['number', '|', 'undefined'], '|', 'null'],
  percentage: 'number'
},
AllIntegerTypes: {
  id: 'string',
  positiveU8: 'number',
  positiveU16: 'number',
  positiveU32: 'number',
  positiveU64: 'number',
  nonNegativeUsize: 'number',
  positiveI8: 'number',
  positiveI16: 'number',
  positiveI32: 'number',
  positiveI64: 'number',
  nonNegativeIsize: 'number'
},
CartItem: {
  productId: 'string',
  productName: 'string',
  quantity: 'number',
  unitPrice: 'number'
},

});


export const defaultMaxValidatorStacking: MaxValidatorStacking = {
id: "",
megaValidatedString: "",
optionalMegaValidated: null,
megaValidatedNumber: 0,
megaValidatedU32: 0,
optionalMegaU32: null,
megaValidatedI64: 0,
optionalMegaI64: null
};
export const defaultCustomer: Customer = {
id: "",
user: '',
shippingAddress: null,
billingAddress: null,
phone: null,
createdAt: ""
};
export const defaultOrder: Order = {
id: "",
customer: '',
items: [],
subtotal: 0,
tax: 0,
shippingCost: 0,
total: 0,
status: '"Delivered",
shippingAddress: { street: "", city: "", state: "", postalCode: "", country: "" },
notes: null,
createdAt: "",
shippedAt: null,
deliveredAt: null
};
export const defaultArrayValidationExtremes: ArrayValidationExtremes = {
id: "",
tags: [],
validatedEmails: [],
optionalTags: null,
scores: [],
optionalScores: null,
measurements: [],
addresses: [],
backupAddresses: null
};
export const defaultDateTimeExtremes: DateTimeExtremes = {
id: "",
createdAt: "",
optionalUpdatedAt: null,
birthDate: "",
optionalExpiryDate: null,
startTime: "",
optionalEndTime: null,
appointment: "",
optionalFollowup: null,
duration: "",
optionalDuration: null,
timezone: "",
optionalTimezone: null,
deadline: "",
optionalTargetDate: null
};
export const defaultNetworkTypes: NetworkTypes = {
id: "",
ipAddress: "",
optionalIp: null,
macAddress: "",
optionalMac: null,
internalApiUrl: "",
optionalStorageUrl: null,
userAgent: "",
optionalUserAgent: null
};
export const defaultNetworkTypes: NetworkTypes = {
id: "",
ipAddress: "",
optionalIp: null,
macAddress: "",
optionalMac: null,
internalApiUrl: "",
optionalStorageUrl: null,
userAgent: "",
optionalUserAgent: null
};
export const defaultSession: Session = {
id: "",
user: '',
token: "",
expiresAt: "",
createdAt: ""
};
export const defaultComplexStringValidation: ComplexStringValidation = {
id: "",
optionalUuid: null,
optionalIp: null,
optionalIpv4: null,
optionalDigits: null,
optionalHex: null,
optionalAlpha: null,
optionalAlphanumeric: null,
optionalDomain: null,
optionalWithAt: null
};
export const defaultFloatValidation: FloatValidation = {
id: "",
positiveFloat: 0,
nonNegativeFloat: 0,
normalized: 0,
optionalPositiveFloat: null
};
export const defaultNonEmptyOptionString: NonEmptyOptionString = {
id: "",
optionalNonEmpty: null,
optionalMinThree: null
};
export const defaultComplexIdentifiers: ComplexIdentifiers = {
id: "",
uuidField: "",
optionalUuid: null,
hexId32: "",
optionalHexId: null,
base64Token: "",
optionalBase64Token: null,
version: "",
optionalVersion: null,
sha256Hash: "",
optionalHash: null
};
export const defaultPost: Post = {
id: "",
title: "",
slug: "",
content: "",
excerpt: null,
author: '',
tags: [],
featuredImage: null,
published: false,
publishedAt: null,
viewCount: 0,
createdAt: "",
updatedAt: ""
};
export const defaultKitchenSinkString: KitchenSinkString = {
id: "",
strictUsername: "",
optionalStrictUsername: null,
validatedEmail: "",
optionalValidatedEmail: null,
apiEndpoint: "",
optionalCdnUrl: null
};
export const defaultComplexIdentifiers: ComplexIdentifiers = {
id: "",
uuidField: "",
optionalUuid: null,
hexId32: "",
optionalHexId: null,
base64Token: "",
optionalBase64Token: null,
version: "",
optionalVersion: null,
sha256Hash: "",
optionalHash: null
};
export const defaultMultipleNumberValidators: MultipleNumberValidators = {
id: "",
boundedPositive: 0,
optionalByte: null,
percentage: 0
};
export const defaultBusinessExtremes: BusinessExtremes = {
id: "",
companyName: "",
jobTitle: "",
optionalParentCompany: null,
productName: "",
sku: "",
optionalVariantName: null,
optionalVariantSku: null,
price: 0,
optionalDiscount: null,
quantity: 0,
optionalMinQuantity: null,
testCardNumber: "",
optionalBackupCard: null
};
export const defaultDeepNestedValidation: DeepNestedValidation = {
id: "",
name: "",
primaryAddress: { street: "", city: "", stateCode: "", postalCode: "", countryCode: "", latitude: null, longitude: null },
billingAddress: null,
shippingAddresses: [],
primaryContact: { name: "", email: "", phone: null, title: null },
secondaryContact: null,
additionalContacts: [],
totalOrders: 0,
totalSpent: 0,
creditLimit: null
};
export const defaultEdgeCasePost: EdgeCasePost = {
id: "",
author: '',
title: "",
content: "",
slug: "",
createdAt: "",
updatedAt: null,
viewCount: 0,
likeCount: 0,
isPublished: false,
featuredImage: null,
metaDescription: null
};
export const defaultOptionalIntegerValidation: OptionalIntegerValidation = {
id: "",
optionalPositiveU32: null,
optionalNonNegativeI32: null,
optionalPositiveU64: null,
optionalNegativeI64: null
};
export const defaultEdgeCasePost: EdgeCasePost = {
id: "",
author: '',
title: "",
content: "",
slug: "",
createdAt: "",
updatedAt: null,
viewCount: 0,
likeCount: 0,
isPublished: false,
featuredImage: null,
metaDescription: null
};
export const defaultProduct: Product = {
id: "",
name: "",
description: "",
price: 0,
stockQuantity: 0,
category: '"Sports",
imageUrl: null,
isAvailable: false,
createdAt: ""
};
export const defaultExtremeIntegerValidation: ExtremeIntegerValidation = {
id: "",
constrainedU8: 0,
optionalConstrainedU8: null,
boundedU16: 0,
optionalBoundedU16: null,
percentageU32: 0,
optionalPercentageU32: null,
largeU64: 0,
optionalLargeU64: null,
negativeI8: 0,
optionalNegativeI8: null,
nonPositiveI16: 0,
optionalNonPositiveI16: null,
boundedI32: 0,
optionalBoundedI32: null,
constrainedI64: 0,
optionalNonNegativeI64: null,
boundedUsize: 0,
optionalBoundedUsize: null,
boundedIsize: 0,
optionalPositiveIsize: null
};
export const defaultUser: User = {
id: "",
email: "",
username: "",
passwordHash: "",
roles: [],
isActive: false,
createdAt: "",
updatedAt: ""
};
export const defaultTag: Tag = {
id: "",
name: "",
slug: "",
description: null,
postCount: 0
};
export const defaultEdgeCaseUser: EdgeCaseUser = {
id: "",
email: "",
username: "",
createdAt: "",
loginCount: 0,
rating: null
};
export const defaultValidatedAddress: ValidatedAddress = {
street: "",
city: "",
stateCode: "",
postalCode: "",
countryCode: "",
latitude: null,
longitude: null
};
export const defaultPersonalInfoExtremes: PersonalInfoExtremes = {
id: "",
firstName: "",
lastName: "",
fullName: "",
optionalNickname: null,
phone: "",
optionalMobile: null,
email: "",
optionalWorkEmail: null,
street: "",
city: "",
state: "",
postalCode: "",
country: "",
optionalStreet: null,
optionalCity: null
};
export const defaultDeepNestedValidation: DeepNestedValidation = {
id: "",
name: "",
primaryAddress: { street: "", city: "", stateCode: "", postalCode: "", countryCode: "", latitude: null, longitude: null },
billingAddress: null,
shippingAddresses: [],
primaryContact: { name: "", email: "", phone: null, title: null },
secondaryContact: null,
additionalContacts: [],
totalOrders: 0,
totalSpent: 0,
creditLimit: null
};
export const defaultOptionalIntegerValidation: OptionalIntegerValidation = {
id: "",
optionalPositiveU32: null,
optionalNonNegativeI32: null,
optionalPositiveU64: null,
optionalNegativeI64: null
};
export const defaultComment: Comment = {
id: "",
post: '',
author: '',
content: "",
parentCommentId: null,
isApproved: false,
createdAt: "",
editedAt: null
};
export const defaultComplexPayment: ComplexPayment = {
id: "",
user: '',
status: '"Refunded",
method: '"DebitCard",
amount: 0,
currency: "",
fee: 0,
tax: 0,
total: 0,
transactionId: "",
referenceCode: null,
createdAt: "",
processedAt: null,
completedAt: null,
billingAddress: { street: "", city: "", stateCode: "", postalCode: "", countryCode: "", latitude: null, longitude: null },
notes: null,
retryCount: 0,
failureReason: null
};
export const defaultFloatValidation: FloatValidation = {
id: "",
positiveFloat: 0,
nonNegativeFloat: 0,
normalized: 0,
optionalPositiveFloat: null
};
export const defaultSession: Session = {
id: "",
user: '',
token: "",
expiresAt: "",
createdAt: ""
};
export const defaultExtremeIntegerValidation: ExtremeIntegerValidation = {
id: "",
constrainedU8: 0,
optionalConstrainedU8: null,
boundedU16: 0,
optionalBoundedU16: null,
percentageU32: 0,
optionalPercentageU32: null,
largeU64: 0,
optionalLargeU64: null,
negativeI8: 0,
optionalNegativeI8: null,
nonPositiveI16: 0,
optionalNonPositiveI16: null,
boundedI32: 0,
optionalBoundedI32: null,
constrainedI64: 0,
optionalNonNegativeI64: null,
boundedUsize: 0,
optionalBoundedUsize: null,
boundedIsize: 0,
optionalPositiveIsize: null
};
export const defaultCaseValidation: CaseValidation = {
id: "",
optionalLowercase: null,
optionalUppercase: null,
optionalCapitalized: null,
optionalUncapitalized: null,
optionalTrimmed: null
};
export const defaultIntegerBounds: IntegerBounds = {
id: "",
percentage: 0,
aboveZero: 0,
underThousand: 0,
optionalByteValue: null,
optionalBounded: null
};
export const defaultMultiValidatorOptionalString: MultiValidatorOptionalString = {
id: "",
username: null,
email: null,
website: null
};
export const defaultInnerValidated: InnerValidated = {
name: "",
count: 0,
optionalEmail: null
};
export const defaultAddress: Address = {
street: "",
city: "",
state: "",
postalCode: "",
country: ""
};
export const defaultEdgeCaseComment: EdgeCaseComment = {
id: "",
post: '',
author: '',
content: "",
createdAt: "",
editedAt: null,
likeCount: 0,
parentCommentId: null,
isApproved: false,
authorIp: null
};
export const defaultExtremeFloatValidation: ExtremeFloatValidation = {
id: "",
multiConstrainedF32: 0,
optionalF32: null,
normalizedF64: 0,
optionalNormalizedF64: null,
negativeF64: 0,
optionalNegativeF64: null,
currencyAmount: 0,
optionalCurrency: null,
percentage: 0,
optionalPercentage: null,
latitude: 0,
longitude: 0,
optionalLatitude: null,
optionalLongitude: null
};
export const defaultBusinessExtremes: BusinessExtremes = {
id: "",
companyName: "",
jobTitle: "",
optionalParentCompany: null,
productName: "",
sku: "",
optionalVariantName: null,
optionalVariantSku: null,
price: 0,
optionalDiscount: null,
quantity: 0,
optionalMinQuantity: null,
testCardNumber: "",
optionalBackupCard: null
};
export const defaultPost: Post = {
id: "",
title: "",
slug: "",
content: "",
excerpt: null,
author: '',
tags: [],
featuredImage: null,
published: false,
publishedAt: null,
viewCount: 0,
createdAt: "",
updatedAt: ""
};
export const defaultCustomer: Customer = {
id: "",
user: '',
shippingAddress: null,
billingAddress: null,
phone: null,
createdAt: ""
};
export const defaultMaxValidatorStacking: MaxValidatorStacking = {
id: "",
megaValidatedString: "",
optionalMegaValidated: null,
megaValidatedNumber: 0,
megaValidatedU32: 0,
optionalMegaU32: null,
megaValidatedI64: 0,
optionalMegaI64: null
};
export const defaultUser: User = {
id: "",
email: "",
username: "",
passwordHash: "",
roles: [],
isActive: false,
createdAt: "",
updatedAt: ""
};
export const defaultProduct: Product = {
id: "",
name: "",
description: "",
price: 0,
stockQuantity: 0,
category: '"Electronics",
imageUrl: null,
isAvailable: false,
createdAt: ""
};
export const defaultAuthor: Author = {
id: "",
user: '',
bio: null,
avatarUrl: null,
twitterHandle: null,
githubHandle: null,
createdAt: ""
};
export const defaultOrder: Order = {
id: "",
customer: '',
items: [],
subtotal: 0,
tax: 0,
shippingCost: 0,
total: 0,
status: '"Shipped",
shippingAddress: { street: "", city: "", state: "", postalCode: "", country: "" },
notes: null,
createdAt: "",
shippedAt: null,
deliveredAt: null
};
export const defaultTag: Tag = {
id: "",
name: "",
slug: "",
description: null,
postCount: 0
};
export const defaultComment: Comment = {
id: "",
post: '',
author: '',
content: "",
parentCommentId: null,
isApproved: false,
createdAt: "",
editedAt: null
};
export const defaultCaseValidation: CaseValidation = {
id: "",
optionalLowercase: null,
optionalUppercase: null,
optionalCapitalized: null,
optionalUncapitalized: null,
optionalTrimmed: null
};
export const defaultValidatedContact: ValidatedContact = {
name: "",
email: "",
phone: null,
title: null
};
export const defaultNonEmptyOptionString: NonEmptyOptionString = {
id: "",
optionalNonEmpty: null,
optionalMinThree: null
};
export const defaultMultiValidatorOptionalString: MultiValidatorOptionalString = {
id: "",
username: null,
email: null,
website: null
};
export const defaultAuthor: Author = {
id: "",
user: '',
bio: null,
avatarUrl: null,
twitterHandle: null,
githubHandle: null,
createdAt: ""
};
export const defaultComplexPayment: ComplexPayment = {
id: "",
user: '',
status: '"Cancelled",
method: '"Cash",
amount: 0,
currency: "",
fee: 0,
tax: 0,
total: 0,
transactionId: "",
referenceCode: null,
createdAt: "",
processedAt: null,
completedAt: null,
billingAddress: { street: "", city: "", stateCode: "", postalCode: "", countryCode: "", latitude: null, longitude: null },
notes: null,
retryCount: 0,
failureReason: null
};
export const defaultAllIntegerTypes: AllIntegerTypes = {
id: "",
positiveU8: 0,
positiveU16: 0,
positiveU32: 0,
positiveU64: 0,
nonNegativeUsize: 0,
positiveI8: 0,
positiveI16: 0,
positiveI32: 0,
positiveI64: 0,
nonNegativeIsize: 0
};
export const defaultDateTimeExtremes: DateTimeExtremes = {
id: "",
createdAt: "",
optionalUpdatedAt: null,
birthDate: "",
optionalExpiryDate: null,
startTime: "",
optionalEndTime: null,
appointment: "",
optionalFollowup: null,
duration: "",
optionalDuration: null,
timezone: "",
optionalTimezone: null,
deadline: "",
optionalTargetDate: null
};
export const defaultPersonalInfoExtremes: PersonalInfoExtremes = {
id: "",
firstName: "",
lastName: "",
fullName: "",
optionalNickname: null,
phone: "",
optionalMobile: null,
email: "",
optionalWorkEmail: null,
street: "",
city: "",
state: "",
postalCode: "",
country: "",
optionalStreet: null,
optionalCity: null
};
export const defaultExtremeFloatValidation: ExtremeFloatValidation = {
id: "",
multiConstrainedF32: 0,
optionalF32: null,
normalizedF64: 0,
optionalNormalizedF64: null,
negativeF64: 0,
optionalNegativeF64: null,
currencyAmount: 0,
optionalCurrency: null,
percentage: 0,
optionalPercentage: null,
latitude: 0,
longitude: 0,
optionalLatitude: null,
optionalLongitude: null
};
export const defaultComplexStringValidation: ComplexStringValidation = {
id: "",
optionalUuid: null,
optionalIp: null,
optionalIpv4: null,
optionalDigits: null,
optionalHex: null,
optionalAlpha: null,
optionalAlphanumeric: null,
optionalDomain: null,
optionalWithAt: null
};
export const defaultOuterWithNestedValidator: OuterWithNestedValidator = {
id: "",
inner: { name: "", count: 0, optionalEmail: null },
optionalInner: null,
innerList: []
};
export const defaultEdgeCaseUser: EdgeCaseUser = {
id: "",
email: "",
username: "",
createdAt: "",
loginCount: 0,
rating: null
};
export const defaultKitchenSinkString: KitchenSinkString = {
id: "",
strictUsername: "",
optionalStrictUsername: null,
validatedEmail: "",
optionalValidatedEmail: null,
apiEndpoint: "",
optionalCdnUrl: null
};
export const defaultArrayValidationExtremes: ArrayValidationExtremes = {
id: "",
tags: [],
validatedEmails: [],
optionalTags: null,
scores: [],
optionalScores: null,
measurements: [],
addresses: [],
backupAddresses: null
};
export const defaultIntegerBounds: IntegerBounds = {
id: "",
percentage: 0,
aboveZero: 0,
underThousand: 0,
optionalByteValue: null,
optionalBounded: null
};
export const defaultOuterWithNestedValidator: OuterWithNestedValidator = {
id: "",
inner: { name: "", count: 0, optionalEmail: null },
optionalInner: null,
innerList: []
};
export const defaultEdgeCaseComment: EdgeCaseComment = {
id: "",
post: '',
author: '',
content: "",
createdAt: "",
editedAt: null,
likeCount: 0,
parentCommentId: null,
isApproved: false,
authorIp: null
};
export const defaultMultipleNumberValidators: MultipleNumberValidators = {
id: "",
boundedPositive: 0,
optionalByte: null,
percentage: 0
};
export const defaultAllIntegerTypes: AllIntegerTypes = {
id: "",
positiveU8: 0,
positiveU16: 0,
positiveU32: 0,
positiveU64: 0,
nonNegativeUsize: 0,
positiveI8: 0,
positiveI16: 0,
positiveI32: 0,
positiveI64: 0,
nonNegativeIsize: 0
};
export const defaultCartItem: CartItem = {
productId: "",
productName: "",
quantity: 0,
unitPrice: 0
};


 export const validator = scope({
  ...bindings.export(),
            }).export();