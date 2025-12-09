import { Schema } from "effect";

export class Address extends Schema.Class<Address>("Address")( { 
  street: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Street` Please enter a value` })).pipe(Schema.maxLength(200, { message: () => `Street` must be at most 200 characters long` }))).annotations({ missingMessage: () => `'Street' is required` }),
  city: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `City` Please enter a value` })).pipe(Schema.maxLength(100, { message: () => `City` must be at most 100 characters long` }))).annotations({ missingMessage: () => `'City' is required` }),
  state: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.maxLength(100, { message: () => `State` must be at most 100 characters long` }))).annotations({ missingMessage: () => `'State' is required` }),
  postalCode: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Postal Code` Please enter a value` })).pipe(Schema.maxLength(20, { message: () => `Postal Code` must be at most 20 characters long` }))).annotations({ missingMessage: () => `'Postal Code' is required` }),
  country: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Country` Please enter a value` })).pipe(Schema.minLength(2, { message: () => `Country` must be at least 2 characters long` })).pipe(Schema.maxLength(2, { message: () => `Country` must be at most 2 characters long` })).pipe(Schema.toUpperCase).annotations({ missingMessage: () => `'Country' is required` })
}) {[key: string]: unknown}

export const Role = Schema.Union(Schema.Literal("Admin"), Schema.Literal("Moderator"), Schema.Literal("User"), Schema.Literal("Guest")).annotations({ identifier: `Role` });
export class User extends Schema.Class<User>("User")( { 
  id: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Id' is required` }),
  email: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.minLength(5, { message: () => `Email` must be at least 5 characters long` })).pipe(Schema.maxLength(255, { message: () => `Email` must be at most 255 characters long` }))).annotations({ missingMessage: () => `'Email' is required` }),
  username: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.minLength(3, { message: () => `Username` must be at least 3 characters long` })).pipe(Schema.maxLength(50, { message: () => `Username` must be at most 50 characters long` }))).annotations({ missingMessage: () => `'Username' is required` }),
  passwordHash: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Password Hash` Please enter a value` }))).annotations({ missingMessage: () => `'Password Hash' is required` }),
  roles: Schema.propertySignature(Schema.Array(Role)).annotations({ missingMessage: () => `'Roles' is required` }),
  isActive: Schema.propertySignature(Schema.Boolean).annotations({ missingMessage: () => `'Is Active' is required` }),
  createdAt: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Created At' is required` }),
  updatedAt: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Updated At' is required` })
}) {[key: string]: unknown}

export class Customer extends Schema.Class<Customer>("Customer")( { 
  id: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Id' is required` }),
  user: Schema.propertySignature(Schema.Union(Schema.String.pipe(Schema.nonEmptyString()), User).annotations({ message: () => ({
                message: `Please enter a valid value`,
                override: true,
            }), })).annotations({ missingMessage: () => `'User' is required` }),
  shippingAddress: Schema.OptionFromNullishOr(Address, null),
  billingAddress: Schema.OptionFromNullishOr(Address, null),
  phone: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null),
  createdAt: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Created At' is required` })
}) {[key: string]: unknown}

export class Tag extends Schema.Class<Tag>("Tag")( { 
  id: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Id' is required` }),
  name: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Name` Please enter a value` })).pipe(Schema.maxLength(100, { message: () => `Name` must be at most 100 characters long` }))).annotations({ missingMessage: () => `'Name' is required` }),
  slug: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Slug` Please enter a value` })).pipe(Schema.maxLength(100, { message: () => `Slug` must be at most 100 characters long` })).pipe(Schema.toLowerCase).annotations({ missingMessage: () => `'Slug' is required` }),
  description: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null).pipe(Schema.maxLength(500, { message: () => `Description` must be at most 500 characters long` })),
  postCount: Schema.propertySignature(Schema.Number.pipe(Schema.nonNegative({ message: () => `Post Count` must be a non-negative number` }))).annotations({ missingMessage: () => `'Post Count' is required` })
}) {[key: string]: unknown}

export class Author extends Schema.Class<Author>("Author")( { 
  id: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Id' is required` }),
  user: Schema.propertySignature(Schema.Union(Schema.String.pipe(Schema.nonEmptyString()), User).annotations({ message: () => ({
                message: `Please enter a valid value`,
                override: true,
            }), })).annotations({ missingMessage: () => `'User' is required` }),
  bio: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null).pipe(Schema.maxLength(2000, { message: () => `Bio` must be at most 2000 characters long` })),
  avatarUrl: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null),
  twitterHandle: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null).pipe(Schema.startsWith("@", { message: () => `Twitter Handle` must start with "@" }).pipe(Schema.maxLength(16, { message: () => `Twitter Handle` must be at most 16 characters long` })),
  githubHandle: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null).pipe(Schema.maxLength(39, { message: () => `Github Handle` must be at most 39 characters long` })),
  createdAt: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Created At' is required` })
}) {[key: string]: unknown}

export class Post extends Schema.Class<Post>("Post")( { 
  id: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Id' is required` }),
  title: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Title` Please enter a value` })).pipe(Schema.minLength(1, { message: () => `Title` must be at least 1 characters long` })).pipe(Schema.maxLength(200, { message: () => `Title` must be at most 200 characters long` }))).annotations({ missingMessage: () => `'Title' is required` }),
  slug: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Slug` Please enter a value` })).pipe(Schema.maxLength(250, { message: () => `Slug` must be at most 250 characters long` })).pipe(Schema.toLowerCase).annotations({ missingMessage: () => `'Slug' is required` }),
  content: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Content` Please enter a value` }))).annotations({ missingMessage: () => `'Content' is required` }),
  excerpt: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null).pipe(Schema.maxLength(500, { message: () => `Excerpt` must be at most 500 characters long` })),
  author: Schema.propertySignature(Schema.Union(Schema.String.pipe(Schema.nonEmptyString()), Author).annotations({ message: () => ({
                message: `Please enter a valid value`,
                override: true,
            }), })).annotations({ missingMessage: () => `'Author' is required` }),
  tags: Schema.propertySignature(Schema.Array(Schema.Union(Schema.String.pipe(Schema.nonEmptyString()), Tag).annotations({ message: () => ({
                message: `Please enter a valid value`,
                override: true,
            }), }))).annotations({ missingMessage: () => `'Tags' is required` }),
  featuredImage: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null),
  published: Schema.propertySignature(Schema.Boolean).annotations({ missingMessage: () => `'Published' is required` }),
  publishedAt: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null),
  viewCount: Schema.propertySignature(Schema.Number.pipe(Schema.nonNegative({ message: () => `View Count` must be a non-negative number` }))).annotations({ missingMessage: () => `'View Count' is required` }),
  createdAt: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Created At' is required` }),
  updatedAt: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Updated At' is required` })
}) {[key: string]: unknown}

export class EdgeCaseUser extends Schema.Class<EdgeCaseUser>("EdgeCaseUser")( { 
  id: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Id' is required` }),
  email: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.maxLength(255, { message: () => `Email` must be at most 255 characters long` }))).annotations({ missingMessage: () => `'Email' is required` }),
  username: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.minLength(3, { message: () => `Username` must be at least 3 characters long` })).pipe(Schema.maxLength(30, { message: () => `Username` must be at most 30 characters long` }))).annotations({ missingMessage: () => `'Username' is required` }),
  createdAt: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Created At' is required` }),
  loginCount: Schema.propertySignature(Schema.Number.pipe(Schema.nonNegative({ message: () => `Login Count` must be a non-negative number` }))).annotations({ missingMessage: () => `'Login Count' is required` }),
  rating: Schema.OptionFromNullishOr(Schema.Number, null).pipe(Schema.between(0, 5, { message: () => `Rating` must be between 0 and 5` }))
}) {[key: string]: unknown}

export class EdgeCasePost extends Schema.Class<EdgeCasePost>("EdgeCasePost")( { 
  id: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Id' is required` }),
  author: Schema.propertySignature(Schema.Union(Schema.String.pipe(Schema.nonEmptyString()), EdgeCaseUser).annotations({ message: () => ({
                message: `Please enter a valid value`,
                override: true,
            }), })).annotations({ missingMessage: () => `'Author' is required` }),
  title: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Title` Please enter a value` })).pipe(Schema.minLength(5, { message: () => `Title` must be at least 5 characters long` })).pipe(Schema.maxLength(200, { message: () => `Title` must be at most 200 characters long` }))).annotations({ missingMessage: () => `'Title' is required` }),
  content: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Content` Please enter a value` })).pipe(Schema.minLength(10, { message: () => `Content` must be at least 10 characters long` }))).annotations({ missingMessage: () => `'Content' is required` }),
  slug: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.toLowerCase.pipe(Schema.maxLength(100, { message: () => `Slug` must be at most 100 characters long` }))).annotations({ missingMessage: () => `'Slug' is required` }),
  createdAt: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Created At' is required` }),
  updatedAt: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null),
  viewCount: Schema.propertySignature(Schema.Number.pipe(Schema.nonNegative({ message: () => `View Count` must be a non-negative number` }))).annotations({ missingMessage: () => `'View Count' is required` }),
  likeCount: Schema.propertySignature(Schema.Number.pipe(Schema.nonNegative({ message: () => `Like Count` must be a non-negative number` }))).annotations({ missingMessage: () => `'Like Count' is required` }),
  isPublished: Schema.propertySignature(Schema.Boolean).annotations({ missingMessage: () => `'Is Published' is required` }),
  featuredImage: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null),
  metaDescription: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null).pipe(Schema.maxLength(300, { message: () => `Meta Description` must be at most 300 characters long` }))
}) {[key: string]: unknown}

export class EdgeCaseComment extends Schema.Class<EdgeCaseComment>("EdgeCaseComment")( { 
  id: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Id' is required` }),
  post: Schema.propertySignature(Schema.Union(Schema.String.pipe(Schema.nonEmptyString()), EdgeCasePost).annotations({ message: () => ({
                message: `Please enter a valid value`,
                override: true,
            }), })).annotations({ missingMessage: () => `'Post' is required` }),
  author: Schema.propertySignature(Schema.Union(Schema.String.pipe(Schema.nonEmptyString()), EdgeCaseUser).annotations({ message: () => ({
                message: `Please enter a valid value`,
                override: true,
            }), })).annotations({ missingMessage: () => `'Author' is required` }),
  content: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Content` Please enter a value` })).pipe(Schema.minLength(1, { message: () => `Content` must be at least 1 characters long` })).pipe(Schema.maxLength(10000, { message: () => `Content` must be at most 10000 characters long` }))).annotations({ missingMessage: () => `'Content' is required` }),
  createdAt: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Created At' is required` }),
  editedAt: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null),
  likeCount: Schema.propertySignature(Schema.Number.pipe(Schema.nonNegative({ message: () => `Like Count` must be a non-negative number` }))).annotations({ missingMessage: () => `'Like Count' is required` }),
  parentCommentId: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null),
  isApproved: Schema.propertySignature(Schema.Boolean).annotations({ missingMessage: () => `'Is Approved' is required` }),
  authorIp: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null)
}) {[key: string]: unknown}

export class ValidatedAddress extends Schema.Class<ValidatedAddress>("ValidatedAddress")( { 
  street: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Street` Please enter a value` })).pipe(Schema.maxLength(200, { message: () => `Street` must be at most 200 characters long` }))).annotations({ missingMessage: () => `'Street' is required` }),
  city: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `City` Please enter a value` })).pipe(Schema.maxLength(100, { message: () => `City` must be at most 100 characters long` }))).annotations({ missingMessage: () => `'City' is required` }),
  stateCode: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.toUpperCase.pipe(Schema.minLength(2, { message: () => `State Code` must be at least 2 characters long` })).pipe(Schema.maxLength(3, { message: () => `State Code` must be at most 3 characters long` }))).annotations({ missingMessage: () => `'State Code' is required` }),
  postalCode: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Postal Code` Please enter a value` })).pipe(Schema.maxLength(20, { message: () => `Postal Code` must be at most 20 characters long` }))).annotations({ missingMessage: () => `'Postal Code' is required` }),
  countryCode: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.toUpperCase.pipe(Schema.minLength(2, { message: () => `Country Code` must be at least 2 characters long` })).pipe(Schema.maxLength(2, { message: () => `Country Code` must be at most 2 characters long` }))).annotations({ missingMessage: () => `'Country Code' is required` }),
  latitude: Schema.OptionFromNullishOr(Schema.Number, null).pipe(Schema.between(-90, 90, { message: () => `Latitude` must be between -90 and 90` })),
  longitude: Schema.OptionFromNullishOr(Schema.Number, null).pipe(Schema.between(-180, 180, { message: () => `Longitude` must be between -180 and 180` }))
}) {[key: string]: unknown}

export class ArrayValidationExtremes extends Schema.Class<ArrayValidationExtremes>("ArrayValidationExtremes")( { 
  id: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Id' is required` }),
  tags: Schema.propertySignature(Schema.Array(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })))).annotations({ missingMessage: () => `'Tags' is required` }),
  validatedEmails: Schema.propertySignature(Schema.Array(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })))).annotations({ missingMessage: () => `'Validated Emails' is required` }),
  optionalTags: Schema.OptionFromNullishOr(Schema.Array(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))), null),
  scores: Schema.propertySignature(Schema.Array(Schema.Number)).annotations({ missingMessage: () => `'Scores' is required` }),
  optionalScores: Schema.OptionFromNullishOr(Schema.Array(Schema.Number), null),
  measurements: Schema.propertySignature(Schema.Array(Schema.Number)).annotations({ missingMessage: () => `'Measurements' is required` }),
  addresses: Schema.propertySignature(Schema.Array(ValidatedAddress)).annotations({ missingMessage: () => `'Addresses' is required` }),
  backupAddresses: Schema.OptionFromNullishOr(Schema.Array(ValidatedAddress), null)
}) {[key: string]: unknown}

export class Comment extends Schema.Class<Comment>("Comment")( { 
  id: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Id' is required` }),
  post: Schema.propertySignature(Schema.Union(Schema.String.pipe(Schema.nonEmptyString()), Post).annotations({ message: () => ({
                message: `Please enter a valid value`,
                override: true,
            }), })).annotations({ missingMessage: () => `'Post' is required` }),
  author: Schema.propertySignature(Schema.Union(Schema.String.pipe(Schema.nonEmptyString()), Author).annotations({ message: () => ({
                message: `Please enter a valid value`,
                override: true,
            }), })).annotations({ missingMessage: () => `'Author' is required` }),
  content: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Content` Please enter a value` })).pipe(Schema.minLength(1, { message: () => `Content` must be at least 1 characters long` })).pipe(Schema.maxLength(5000, { message: () => `Content` must be at most 5000 characters long` }))).annotations({ missingMessage: () => `'Content' is required` }),
  parentCommentId: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null),
  isApproved: Schema.propertySignature(Schema.Boolean).annotations({ missingMessage: () => `'Is Approved' is required` }),
  createdAt: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Created At' is required` }),
  editedAt: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null)
}) {[key: string]: unknown}

export class CartItem extends Schema.Class<CartItem>("CartItem")( { 
  productId: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Product Id` Please enter a value` }))).annotations({ missingMessage: () => `'Product Id' is required` }),
  productName: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Product Name` Please enter a value` })).pipe(Schema.maxLength(200, { message: () => `Product Name` must be at most 200 characters long` }))).annotations({ missingMessage: () => `'Product Name' is required` }),
  quantity: Schema.propertySignature(Schema.Number.pipe(Schema.positive({ message: () => `Quantity` must be a positive number` }))).annotations({ missingMessage: () => `'Quantity' is required` }),
  unitPrice: Schema.propertySignature(Schema.Number.pipe(Schema.positive({ message: () => `Unit Price` must be a positive number` }))).annotations({ missingMessage: () => `'Unit Price' is required` })
}) {[key: string]: unknown}

export const OrderStatus = Schema.Union(Schema.Literal("Pending"), Schema.Literal("Processing"), Schema.Literal("Shipped"), Schema.Literal("Delivered"), Schema.Literal("Cancelled")).annotations({ identifier: `OrderStatus` });
export class Order extends Schema.Class<Order>("Order")( { 
  id: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Id' is required` }),
  customer: Schema.propertySignature(Schema.Union(Schema.String.pipe(Schema.nonEmptyString()), Customer).annotations({ message: () => ({
                message: `Please enter a valid value`,
                override: true,
            }), })).annotations({ missingMessage: () => `'Customer' is required` }),
  items: Schema.propertySignature(Schema.Array(CartItem)).annotations({ missingMessage: () => `'Items' is required` }),
  subtotal: Schema.propertySignature(Schema.Number.pipe(Schema.nonNegative({ message: () => `Subtotal` must be a non-negative number` }))).annotations({ missingMessage: () => `'Subtotal' is required` }),
  tax: Schema.propertySignature(Schema.Number.pipe(Schema.nonNegative({ message: () => `Tax` must be a non-negative number` }))).annotations({ missingMessage: () => `'Tax' is required` }),
  shippingCost: Schema.propertySignature(Schema.Number.pipe(Schema.nonNegative({ message: () => `Shipping Cost` must be a non-negative number` }))).annotations({ missingMessage: () => `'Shipping Cost' is required` }),
  total: Schema.propertySignature(Schema.Number.pipe(Schema.positive({ message: () => `Total` must be a positive number` }))).annotations({ missingMessage: () => `'Total' is required` }),
  status: Schema.propertySignature(OrderStatus).annotations({ missingMessage: () => `'Status' is required` }),
  shippingAddress: Schema.propertySignature(Address).annotations({ missingMessage: () => `'Shipping Address' is required` }),
  notes: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null).pipe(Schema.maxLength(1000, { message: () => `Notes` must be at most 1000 characters long` })),
  createdAt: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Created At' is required` }),
  shippedAt: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null),
  deliveredAt: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null)
}) {[key: string]: unknown}

export const PaymentStatus = Schema.Union(Schema.Literal("Pending"), Schema.Literal("Processing"), Schema.Literal("Completed"), Schema.Literal("Failed"), Schema.Literal("Refunded"), Schema.Literal("Cancelled")).annotations({ identifier: `PaymentStatus` });
export const PaymentMethod = Schema.Union(Schema.Literal("CreditCard"), Schema.Literal("DebitCard"), Schema.Literal("BankTransfer"), Schema.Literal("PayPal"), Schema.Literal("Crypto"), Schema.Literal("Cash")).annotations({ identifier: `PaymentMethod` });
export class ComplexPayment extends Schema.Class<ComplexPayment>("ComplexPayment")( { 
  id: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Id' is required` }),
  user: Schema.propertySignature(Schema.Union(Schema.String.pipe(Schema.nonEmptyString()), EdgeCaseUser).annotations({ message: () => ({
                message: `Please enter a valid value`,
                override: true,
            }), })).annotations({ missingMessage: () => `'User' is required` }),
  status: Schema.propertySignature(PaymentStatus).annotations({ missingMessage: () => `'Status' is required` }),
  method: Schema.propertySignature(PaymentMethod).annotations({ missingMessage: () => `'Method' is required` }),
  amount: Schema.propertySignature(Schema.Number.pipe(Schema.positive({ message: () => `Amount` must be a positive number` })).pipe(Schema.lessThan(1000000, { message: () => `Amount` must be less than 1000000` }))).annotations({ missingMessage: () => `'Amount' is required` }),
  currency: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.toUpperCase.pipe(Schema.minLength(3, { message: () => `Currency` must be at least 3 characters long` })).pipe(Schema.maxLength(3, { message: () => `Currency` must be at most 3 characters long` }))).annotations({ missingMessage: () => `'Currency' is required` }),
  fee: Schema.propertySignature(Schema.Number.pipe(Schema.nonNegative({ message: () => `Fee` must be a non-negative number` }))).annotations({ missingMessage: () => `'Fee' is required` }),
  tax: Schema.propertySignature(Schema.Number.pipe(Schema.nonNegative({ message: () => `Tax` must be a non-negative number` }))).annotations({ missingMessage: () => `'Tax' is required` }),
  total: Schema.propertySignature(Schema.Number.pipe(Schema.positive({ message: () => `Total` must be a positive number` }))).annotations({ missingMessage: () => `'Total' is required` }),
  transactionId: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.minLength(32, { message: () => `Transaction Id` must be at least 32 characters long` }))).annotations({ missingMessage: () => `'Transaction Id' is required` }),
  referenceCode: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null),
  createdAt: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Created At' is required` }),
  processedAt: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null),
  completedAt: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null),
  billingAddress: Schema.propertySignature(ValidatedAddress).annotations({ missingMessage: () => `'Billing Address' is required` }),
  notes: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null).pipe(Schema.maxLength(1000, { message: () => `Notes` must be at most 1000 characters long` })),
  retryCount: Schema.propertySignature(Schema.Number.pipe(Schema.nonNegative({ message: () => `Retry Count` must be a non-negative number` }))).annotations({ missingMessage: () => `'Retry Count' is required` }),
  failureReason: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null).pipe(Schema.maxLength(500, { message: () => `Failure Reason` must be at most 500 characters long` }))
}) {[key: string]: unknown}

export class Session extends Schema.Class<Session>("Session")( { 
  id: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Id' is required` }),
  user: Schema.propertySignature(Schema.Union(Schema.String.pipe(Schema.nonEmptyString()), User).annotations({ message: () => ({
                message: `Please enter a valid value`,
                override: true,
            }), })).annotations({ missingMessage: () => `'User' is required` }),
  token: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Token` Please enter a value` })).pipe(Schema.minLength(32, { message: () => `Token` must be at least 32 characters long` }))).annotations({ missingMessage: () => `'Token' is required` }),
  expiresAt: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Expires At' is required` }),
  createdAt: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Created At' is required` })
}) {[key: string]: unknown}

export class InnerValidated extends Schema.Class<InnerValidated>("InnerValidated")( { 
  name: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Name` Please enter a value` })).pipe(Schema.maxLength(100, { message: () => `Name` must be at most 100 characters long` }))).annotations({ missingMessage: () => `'Name' is required` }),
  count: Schema.propertySignature(Schema.Number.pipe(Schema.positive({ message: () => `Count` must be a positive number` }))).annotations({ missingMessage: () => `'Count' is required` }),
  optionalEmail: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null)
}) {[key: string]: unknown}

export class OuterWithNestedValidator extends Schema.Class<OuterWithNestedValidator>("OuterWithNestedValidator")( { 
  id: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Id' is required` }),
  inner: Schema.propertySignature(InnerValidated).annotations({ missingMessage: () => `'Inner' is required` }),
  optionalInner: Schema.OptionFromNullishOr(InnerValidated, null),
  innerList: Schema.propertySignature(Schema.Array(InnerValidated)).annotations({ missingMessage: () => `'Inner List' is required` })
}) {[key: string]: unknown}

export const ProductCategory = Schema.Union(Schema.Literal("Electronics"), Schema.Literal("Clothing"), Schema.Literal("Books"), Schema.Literal("Home"), Schema.Literal("Sports"), Schema.Literal("Other")).annotations({ identifier: `ProductCategory` });
export class Product extends Schema.Class<Product>("Product")( { 
  id: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Id' is required` }),
  name: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Name` Please enter a value` })).pipe(Schema.maxLength(200, { message: () => `Name` must be at most 200 characters long` }))).annotations({ missingMessage: () => `'Name' is required` }),
  description: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.maxLength(5000, { message: () => `Description` must be at most 5000 characters long` }))).annotations({ missingMessage: () => `'Description' is required` }),
  price: Schema.propertySignature(Schema.Number.pipe(Schema.positive({ message: () => `Price` must be a positive number` }))).annotations({ missingMessage: () => `'Price' is required` }),
  stockQuantity: Schema.propertySignature(Schema.Number.pipe(Schema.nonNegative({ message: () => `Stock Quantity` must be a non-negative number` }))).annotations({ missingMessage: () => `'Stock Quantity' is required` }),
  category: Schema.propertySignature(ProductCategory).annotations({ missingMessage: () => `'Category' is required` }),
  imageUrl: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null),
  isAvailable: Schema.propertySignature(Schema.Boolean).annotations({ missingMessage: () => `'Is Available' is required` }),
  createdAt: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Created At' is required` })
}) {[key: string]: unknown}

export class ValidatedContact extends Schema.Class<ValidatedContact>("ValidatedContact")( { 
  name: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Name` Please enter a value` })).pipe(Schema.maxLength(100, { message: () => `Name` must be at most 100 characters long` }))).annotations({ missingMessage: () => `'Name' is required` }),
  email: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Email' is required` }),
  phone: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null).pipe(Schema.maxLength(20, { message: () => `Phone` must be at most 20 characters long` })),
  title: Schema.OptionFromNullishOr(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })), null).pipe(Schema.maxLength(100, { message: () => `Title` must be at most 100 characters long` }))
}) {[key: string]: unknown}

export class DeepNestedValidation extends Schema.Class<DeepNestedValidation>("DeepNestedValidation")( { 
  id: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))).annotations({ missingMessage: () => `'Id' is required` }),
  name: Schema.propertySignature(Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` })).pipe(Schema.nonEmptyString({ message: () => `Name` Please enter a value` })).pipe(Schema.maxLength(200, { message: () => `Name` must be at most 200 characters long` }))).annotations({ missingMessage: () => `'Name' is required` }),
  primaryAddress: Schema.propertySignature(ValidatedAddress).annotations({ missingMessage: () => `'Primary Address' is required` }),
  billingAddress: Schema.OptionFromNullishOr(ValidatedAddress, null),
  shippingAddresses: Schema.propertySignature(Schema.Array(ValidatedAddress)).annotations({ missingMessage: () => `'Shipping Addresses' is required` }),
  primaryContact: Schema.propertySignature(ValidatedContact).annotations({ missingMessage: () => `'Primary Contact' is required` }),
  secondaryContact: Schema.OptionFromNullishOr(ValidatedContact, null),
  additionalContacts: Schema.propertySignature(Schema.Array(ValidatedContact)).annotations({ missingMessage: () => `'Additional Contacts' is required` }),
  totalOrders: Schema.propertySignature(Schema.Number.pipe(Schema.nonNegative({ message: () => `Total Orders` must be a non-negative number` }))).annotations({ missingMessage: () => `'Total Orders' is required` }),
  totalSpent: Schema.propertySignature(Schema.Number.pipe(Schema.nonNegative({ message: () => `Total Spent` must be a non-negative number` }))).annotations({ missingMessage: () => `'Total Spent' is required` }),
  creditLimit: Schema.OptionFromNullishOr(Schema.Number, null).pipe(Schema.positive({ message: () => `Credit Limit` must be a positive number` }))
}) {[key: string]: unknown}


export interface AddressEncoded {
  readonly street: string;
  readonly city: string;
  readonly state: string;
  readonly postalCode: string;
  readonly country: string;
}

export type RoleEncoded = "Admin" | "Moderator" | "User" | "Guest";

export interface UserEncoded {
  readonly id: string;
  readonly email: string;
  readonly username: string;
  readonly passwordHash: string;
  readonly roles: ReadonlyArray<RoleEncoded>;
  readonly isActive: boolean;
  readonly createdAt: string;
  readonly updatedAt: string;
}

export interface CustomerEncoded {
  readonly id: string;
  readonly user: string | UserEncoded;
  readonly shippingAddress: AddressEncoded | null | undefined;
  readonly billingAddress: AddressEncoded | null | undefined;
  readonly phone: string | null | undefined;
  readonly createdAt: string;
}

export interface TagEncoded {
  readonly id: string;
  readonly name: string;
  readonly slug: string;
  readonly description: string | null | undefined;
  readonly postCount: number;
}

export interface AuthorEncoded {
  readonly id: string;
  readonly user: string | UserEncoded;
  readonly bio: string | null | undefined;
  readonly avatarUrl: string | null | undefined;
  readonly twitterHandle: string | null | undefined;
  readonly githubHandle: string | null | undefined;
  readonly createdAt: string;
}

export interface PostEncoded {
  readonly id: string;
  readonly title: string;
  readonly slug: string;
  readonly content: string;
  readonly excerpt: string | null | undefined;
  readonly author: string | AuthorEncoded;
  readonly tags: ReadonlyArray<string | TagEncoded>;
  readonly featuredImage: string | null | undefined;
  readonly published: boolean;
  readonly publishedAt: string | null | undefined;
  readonly viewCount: number;
  readonly createdAt: string;
  readonly updatedAt: string;
}

export interface EdgeCaseUserEncoded {
  readonly id: string;
  readonly email: string;
  readonly username: string;
  readonly createdAt: string;
  readonly loginCount: number;
  readonly rating: number | null | undefined;
}

export interface EdgeCasePostEncoded {
  readonly id: string;
  readonly author: string | EdgeCaseUserEncoded;
  readonly title: string;
  readonly content: string;
  readonly slug: string;
  readonly createdAt: string;
  readonly updatedAt: string | null | undefined;
  readonly viewCount: number;
  readonly likeCount: number;
  readonly isPublished: boolean;
  readonly featuredImage: string | null | undefined;
  readonly metaDescription: string | null | undefined;
}

export interface EdgeCaseCommentEncoded {
  readonly id: string;
  readonly post: string | EdgeCasePostEncoded;
  readonly author: string | EdgeCaseUserEncoded;
  readonly content: string;
  readonly createdAt: string;
  readonly editedAt: string | null | undefined;
  readonly likeCount: number;
  readonly parentCommentId: string | null | undefined;
  readonly isApproved: boolean;
  readonly authorIp: string | null | undefined;
}

export interface ValidatedAddressEncoded {
  readonly street: string;
  readonly city: string;
  readonly stateCode: string;
  readonly postalCode: string;
  readonly countryCode: string;
  readonly latitude: number | null | undefined;
  readonly longitude: number | null | undefined;
}

export interface ArrayValidationExtremesEncoded {
  readonly id: string;
  readonly tags: ReadonlyArray<string>;
  readonly validatedEmails: ReadonlyArray<string>;
  readonly optionalTags: ReadonlyArray<string> | null | undefined;
  readonly scores: ReadonlyArray<number>;
  readonly optionalScores: ReadonlyArray<number> | null | undefined;
  readonly measurements: ReadonlyArray<number>;
  readonly addresses: ReadonlyArray<ValidatedAddressEncoded>;
  readonly backupAddresses: ReadonlyArray<ValidatedAddressEncoded> | null | undefined;
}

export interface CommentEncoded {
  readonly id: string;
  readonly post: string | PostEncoded;
  readonly author: string | AuthorEncoded;
  readonly content: string;
  readonly parentCommentId: string | null | undefined;
  readonly isApproved: boolean;
  readonly createdAt: string;
  readonly editedAt: string | null | undefined;
}

export interface CartItemEncoded {
  readonly productId: string;
  readonly productName: string;
  readonly quantity: number;
  readonly unitPrice: number;
}

export type OrderStatusEncoded = "Pending" | "Processing" | "Shipped" | "Delivered" | "Cancelled";

export interface OrderEncoded {
  readonly id: string;
  readonly customer: string | CustomerEncoded;
  readonly items: ReadonlyArray<CartItemEncoded>;
  readonly subtotal: number;
  readonly tax: number;
  readonly shippingCost: number;
  readonly total: number;
  readonly status: OrderStatusEncoded;
  readonly shippingAddress: AddressEncoded;
  readonly notes: string | null | undefined;
  readonly createdAt: string;
  readonly shippedAt: string | null | undefined;
  readonly deliveredAt: string | null | undefined;
}

export type PaymentStatusEncoded = "Pending" | "Processing" | "Completed" | "Failed" | "Refunded" | "Cancelled";

export type PaymentMethodEncoded = "CreditCard" | "DebitCard" | "BankTransfer" | "PayPal" | "Crypto" | "Cash";

export interface ComplexPaymentEncoded {
  readonly id: string;
  readonly user: string | EdgeCaseUserEncoded;
  readonly status: PaymentStatusEncoded;
  readonly method: PaymentMethodEncoded;
  readonly amount: number;
  readonly currency: string;
  readonly fee: number;
  readonly tax: number;
  readonly total: number;
  readonly transactionId: string;
  readonly referenceCode: string | null | undefined;
  readonly createdAt: string;
  readonly processedAt: string | null | undefined;
  readonly completedAt: string | null | undefined;
  readonly billingAddress: ValidatedAddressEncoded;
  readonly notes: string | null | undefined;
  readonly retryCount: number;
  readonly failureReason: string | null | undefined;
}

export interface SessionEncoded {
  readonly id: string;
  readonly user: string | UserEncoded;
  readonly token: string;
  readonly expiresAt: string;
  readonly createdAt: string;
}

export interface InnerValidatedEncoded {
  readonly name: string;
  readonly count: number;
  readonly optionalEmail: string | null | undefined;
}

export interface OuterWithNestedValidatorEncoded {
  readonly id: string;
  readonly inner: InnerValidatedEncoded;
  readonly optionalInner: InnerValidatedEncoded | null | undefined;
  readonly innerList: ReadonlyArray<InnerValidatedEncoded>;
}

export type ProductCategoryEncoded = "Electronics" | "Clothing" | "Books" | "Home" | "Sports" | "Other";

export interface ProductEncoded {
  readonly id: string;
  readonly name: string;
  readonly description: string;
  readonly price: number;
  readonly stockQuantity: number;
  readonly category: ProductCategoryEncoded;
  readonly imageUrl: string | null | undefined;
  readonly isAvailable: boolean;
  readonly createdAt: string;
}

export interface ValidatedContactEncoded {
  readonly name: string;
  readonly email: string;
  readonly phone: string | null | undefined;
  readonly title: string | null | undefined;
}

export interface DeepNestedValidationEncoded {
  readonly id: string;
  readonly name: string;
  readonly primaryAddress: ValidatedAddressEncoded;
  readonly billingAddress: ValidatedAddressEncoded | null | undefined;
  readonly shippingAddresses: ReadonlyArray<ValidatedAddressEncoded>;
  readonly primaryContact: ValidatedContactEncoded;
  readonly secondaryContact: ValidatedContactEncoded | null | undefined;
  readonly additionalContacts: ReadonlyArray<ValidatedContactEncoded>;
  readonly totalOrders: number;
  readonly totalSpent: number;
  readonly creditLimit: number | null | undefined;
}

