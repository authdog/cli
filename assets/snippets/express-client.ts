import express from "express"
import { createAuthdog } from "@authdog/express"

const app = express()
const authdog = createAuthdog({ publicKey: process.env.PK_AUTHDOG! })
app.use(authdog.attachSession())
